//! Front-end-neutral import into a ledger set.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::num::TryFromIntError;

use crate::model::{Account, Amount, Posting, Voucher};
use crate::set::LedgerSet;
use crate::store::YearStore;

/// Rows as read from a source year before canonical ids are assigned.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceYear {
    /// Ledger year for these source rows.
    pub year: i32,
    /// Year-local source accounts.
    pub accounts: Vec<Account>,
    /// Source vouchers keyed by source id.
    pub vouchers: Vec<SourceVoucher>,
    /// Source postings keyed by source id.
    pub postings: Vec<SourcePosting>,
    /// Source high-water mark for voucher ids.
    pub next_source_voucher_id: i64,
    /// Source high-water mark for posting ids.
    pub next_source_posting_id: i64,
}

/// A voucher row from an external source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceVoucher {
    /// Source voucher id.
    pub source_id: i64,
    /// ISO-8601 voucher date.
    pub date: String,
    /// Optional source voucher text.
    pub text: Option<String>,
}

/// A posting row from an external source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourcePosting {
    /// Source posting id.
    pub source_id: i64,
    /// Source voucher id this posting belongs to.
    pub source_voucher_id: i64,
    /// Year-local account number.
    pub account: i64,
    /// Signed posting amount.
    pub amount: Amount,
    /// Optional source posting text.
    pub text: Option<String>,
}

/// Failure while importing one source year.
#[derive(Debug)]
pub enum ImportError {
    /// Filesystem failure.
    Io {
        /// Original filesystem error.
        source: Box<std::io::Error>,
    },
    /// SQLite storage failure.
    Store {
        /// Original SQLite error.
        source: Box<rusqlite::Error>,
    },
    /// The set already contains the requested year.
    YearAlreadyImported {
        /// Duplicate ledger year.
        year: i32,
    },
    /// A source voucher id appears more than once.
    DuplicateSourceVoucher {
        /// Duplicate source voucher id.
        source_id: i64,
    },
    /// A source posting id appears more than once.
    DuplicateSourcePosting {
        /// Duplicate source posting id.
        source_id: i64,
    },
    /// A source posting references a voucher that is not present.
    MissingVoucher {
        /// Missing source voucher id.
        source_voucher_id: i64,
    },
    /// A voucher's postings do not sum to zero.
    Unbalanced {
        /// Canonical voucher id.
        voucher: i64,
        /// Signed minor-unit sum for the voucher.
        minor_sum: i64,
    },
    /// Posting sums overflowed the exact minor-unit range.
    BalanceOverflow {
        /// Canonical voucher id.
        voucher: i64,
    },
    /// A source row count cannot fit in the id allocator.
    IdCountOverflow {
        /// Name of the row collection.
        row_kind: &'static str,
        /// Row count that cannot fit in `i64`.
        count: usize,
    },
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::Io { source } => write!(f, "filesystem import failure: {source}"),
            ImportError::Store { source } => write!(f, "SQLite import failure: {source}"),
            ImportError::YearAlreadyImported { year } => {
                write!(f, "ledger year {year} is already imported")
            }
            ImportError::DuplicateSourceVoucher { source_id } => {
                write!(f, "duplicate source voucher id {source_id}")
            }
            ImportError::DuplicateSourcePosting { source_id } => {
                write!(f, "duplicate source posting id {source_id}")
            }
            ImportError::MissingVoucher { source_voucher_id } => {
                write!(f, "missing source voucher id {source_voucher_id}")
            }
            ImportError::Unbalanced { voucher, minor_sum } => {
                write!(
                    f,
                    "voucher {voucher} is unbalanced by {minor_sum} minor units"
                )
            }
            ImportError::BalanceOverflow { voucher } => {
                write!(f, "voucher {voucher} balance overflows minor units")
            }
            ImportError::IdCountOverflow { row_kind, count } => {
                write!(f, "{row_kind} row count {count} cannot fit in i64")
            }
        }
    }
}

impl std::error::Error for ImportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ImportError::Io { source } => Some(source.as_ref()),
            ImportError::Store { source } => Some(source.as_ref()),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ImportError {
    fn from(source: std::io::Error) -> ImportError {
        ImportError::Io {
            source: Box::new(source),
        }
    }
}

impl From<rusqlite::Error> for ImportError {
    fn from(source: rusqlite::Error) -> ImportError {
        ImportError::Store {
            source: Box::new(source),
        }
    }
}

/// Assign canonical ids, validate balance, and write one year file.
///
/// The set cursor is advanced only after the year file and manifest are both
/// written. On the first import into an empty set, the source high-water marks
/// seed the canonical id cursors.
pub fn import_year(set: &mut LedgerSet, src: SourceYear) -> Result<(), ImportError> {
    let SourceYear {
        year,
        accounts,
        vouchers: source_vouchers,
        postings: source_postings,
        next_source_voucher_id,
        next_source_posting_id,
    } = src;

    let mut draft_set = set.clone();
    if draft_set.manifest.years.contains(&year) {
        return Err(ImportError::YearAlreadyImported { year });
    }
    draft_set.manifest.next_voucher_id = draft_set
        .manifest
        .next_voucher_id
        .max(next_source_voucher_id);
    draft_set.manifest.next_posting_id = draft_set
        .manifest
        .next_posting_id
        .max(next_source_posting_id);

    let voucher_count = count_as_i64("voucher", source_vouchers.len())?;
    let posting_count = count_as_i64("posting", source_postings.len())?;
    let voucher_ids = draft_set.alloc_voucher_ids(voucher_count);
    let posting_ids = draft_set.alloc_posting_ids(posting_count);

    let mut voucher_id_by_source = BTreeMap::new();
    let mut vouchers = Vec::with_capacity(source_vouchers.len());
    for (source, canonical_id) in source_vouchers.into_iter().zip(voucher_ids) {
        if voucher_id_by_source
            .insert(source.source_id, canonical_id)
            .is_some()
        {
            return Err(ImportError::DuplicateSourceVoucher {
                source_id: source.source_id,
            });
        }
        vouchers.push(Voucher {
            id: canonical_id,
            source_id: Some(source.source_id),
            date: source.date,
            text: source.text,
        });
    }

    let mut seen_postings = BTreeSet::new();
    let mut balance_by_voucher = BTreeMap::new();
    let mut postings = Vec::with_capacity(source_postings.len());
    for (source, canonical_id) in source_postings.into_iter().zip(posting_ids) {
        if !seen_postings.insert(source.source_id) {
            return Err(ImportError::DuplicateSourcePosting {
                source_id: source.source_id,
            });
        }
        let voucher_id = *voucher_id_by_source.get(&source.source_voucher_id).ok_or(
            ImportError::MissingVoucher {
                source_voucher_id: source.source_voucher_id,
            },
        )?;
        let balance = balance_by_voucher.entry(voucher_id).or_insert(0_i64);
        *balance = balance
            .checked_add(source.amount.0)
            .ok_or(ImportError::BalanceOverflow {
                voucher: voucher_id,
            })?;
        postings.push(Posting {
            id: canonical_id,
            source_id: Some(source.source_id),
            voucher_id,
            account: source.account,
            amount: source.amount,
            text: source.text,
        });
    }

    for voucher in &vouchers {
        let minor_sum = balance_by_voucher.get(&voucher.id).copied().unwrap_or(0);
        if minor_sum != 0 {
            return Err(ImportError::Unbalanced {
                voucher: voucher.id,
                minor_sum,
            });
        }
    }

    write_imported_year(&mut draft_set, year, &accounts, &vouchers, &postings)?;
    *set = draft_set;
    Ok(())
}

fn count_as_i64(row_kind: &'static str, count: usize) -> Result<i64, ImportError> {
    i64::try_from(count)
        .map_err(|_: TryFromIntError| ImportError::IdCountOverflow { row_kind, count })
}

fn write_imported_year(
    set: &mut LedgerSet,
    year: i32,
    accounts: &[Account],
    vouchers: &[Voucher],
    postings: &[Posting],
) -> Result<(), ImportError> {
    let path = set.year_path(year);
    let store = YearStore::create(&path, year)?;
    let result = write_rows(&store, set, year, accounts, vouchers, postings);
    if let Err(err) = result {
        drop(store);
        let _ = fs::remove_file(path);
        return Err(err);
    }
    Ok(())
}

fn write_rows(
    store: &YearStore,
    set: &mut LedgerSet,
    year: i32,
    accounts: &[Account],
    vouchers: &[Voucher],
    postings: &[Posting],
) -> Result<(), ImportError> {
    for account in accounts {
        store.insert_account(account)?;
    }
    for voucher in vouchers {
        store.insert_voucher(voucher)?;
    }
    for posting in postings {
        store.insert_posting(posting)?;
    }
    store.set_id_state("voucher", set.manifest.next_voucher_id)?;
    store.set_id_state("posting", set.manifest.next_posting_id)?;
    set.manifest.years.push(year);
    set.manifest.years.sort_unstable();
    set.save()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_source_year_with_carried_ids() {
        let dir = tempfile::tempdir().unwrap();
        let mut set = LedgerSet::create(dir.path(), "Household").unwrap();

        import_year(&mut set, balanced_source_year(2024, 11_612, 25_471, 1_200)).unwrap();

        assert_eq!(set.manifest.next_voucher_id, 11_613);
        assert_eq!(set.manifest.next_posting_id, 25_473);
        assert_eq!(set.manifest.years, vec![2024]);

        let store = YearStore::open(&set.year_path(2024)).unwrap();
        assert_eq!(
            store.vouchers().unwrap(),
            vec![Voucher {
                id: 11_612,
                source_id: Some(11_612),
                date: "2024-01-31".to_owned(),
                text: Some("Source voucher".to_owned()),
            }]
        );
        assert_eq!(
            store.postings().unwrap(),
            vec![
                Posting {
                    id: 25_471,
                    source_id: Some(25_471),
                    voucher_id: 11_612,
                    account: 1910,
                    amount: Amount(1_200),
                    text: Some("Debit".to_owned()),
                },
                Posting {
                    id: 25_472,
                    source_id: Some(25_472),
                    voucher_id: 11_612,
                    account: 3010,
                    amount: Amount(-1_200),
                    text: Some("Credit".to_owned()),
                },
            ]
        );
        assert_eq!(id_state(&store, "voucher"), 11_613);
        assert_eq!(id_state(&store, "posting"), 25_473);

        let reloaded = LedgerSet::open(dir.path()).unwrap();
        assert_eq!(reloaded.manifest, set.manifest);
    }

    #[test]
    fn later_import_keeps_existing_cursor_when_source_cursor_is_lower() {
        let dir = tempfile::tempdir().unwrap();
        let mut set = LedgerSet::create(dir.path(), "Household").unwrap();

        import_year(&mut set, balanced_source_year(2024, 11_612, 25_471, 1_200)).unwrap();
        import_year(&mut set, balanced_source_year(2025, 20, 30, 800)).unwrap();

        assert_eq!(set.manifest.next_voucher_id, 11_614);
        assert_eq!(set.manifest.next_posting_id, 25_475);
        assert_eq!(set.manifest.years, vec![2024, 2025]);

        let second = YearStore::open(&set.year_path(2025)).unwrap();
        assert_eq!(second.vouchers().unwrap()[0].id, 11_613);
        assert_eq!(second.vouchers().unwrap()[0].source_id, Some(20));
        let postings = second.postings().unwrap();
        assert_eq!(postings[0].id, 25_473);
        assert_eq!(postings[0].source_id, Some(30));
        assert_eq!(postings[1].id, 25_474);
        assert_eq!(postings[1].source_id, Some(31));
        assert_eq!(id_state(&second, "voucher"), 11_614);
        assert_eq!(id_state(&second, "posting"), 25_475);
    }

    #[test]
    fn unbalanced_source_year_is_rejected_without_mutating_set() {
        let dir = tempfile::tempdir().unwrap();
        let mut set = LedgerSet::create(dir.path(), "Household").unwrap();
        let mut source = balanced_source_year(2024, 11_612, 25_471, 1_200);
        source.postings[1].amount = Amount(-1_199);

        let err = import_year(&mut set, source).unwrap_err();

        assert!(matches!(
            err,
            ImportError::Unbalanced {
                voucher: 11_612,
                minor_sum: 1
            }
        ));
        assert_eq!(set.manifest.next_voucher_id, 1);
        assert_eq!(set.manifest.next_posting_id, 1);
        assert!(set.manifest.years.is_empty());
        assert!(!set.year_path(2024).exists());
    }

    fn balanced_source_year(
        year: i32,
        voucher_source_id: i64,
        posting_source_id: i64,
        minor: i64,
    ) -> SourceYear {
        SourceYear {
            year,
            accounts: vec![account(1910, "Cash"), account(3010, "Sales")],
            vouchers: vec![SourceVoucher {
                source_id: voucher_source_id,
                date: format!("{year}-01-31"),
                text: Some("Source voucher".to_owned()),
            }],
            postings: vec![
                SourcePosting {
                    source_id: posting_source_id,
                    source_voucher_id: voucher_source_id,
                    account: 1910,
                    amount: Amount(minor),
                    text: Some("Debit".to_owned()),
                },
                SourcePosting {
                    source_id: posting_source_id + 1,
                    source_voucher_id: voucher_source_id,
                    account: 3010,
                    amount: Amount(-minor),
                    text: Some("Credit".to_owned()),
                },
            ],
            next_source_voucher_id: voucher_source_id,
            next_source_posting_id: posting_source_id,
        }
    }

    fn account(number: i64, name: &str) -> Account {
        Account {
            number,
            name: name.to_owned(),
            note: None,
            sru_plus: None,
            sru_minus: None,
        }
    }

    fn id_state(store: &YearStore, kind: &str) -> i64 {
        store
            .conn
            .query_row("SELECT next FROM id_state WHERE kind = ?1", [kind], |row| {
                row.get(0)
            })
            .unwrap()
    }
}
