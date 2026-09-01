//! Front-end-neutral import into a ledger set.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::num::TryFromIntError;

use crate::model::{Account, Amount, BalanceError, Posting, Voucher, voucher_balance_violations};
use crate::set::{IdAllocationError, LedgerSet};
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
    /// Supplied storage failure.
    Store {
        /// Sanitized storage error.
        source: Box<crate::StoreError>,
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
        /// Number of posting lines attached to the voucher.
        posting_count: usize,
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
    /// Canonical id allocation failed.
    IdAllocation {
        /// Original allocation error.
        source: IdAllocationError,
    },
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::Store { source } => write!(f, "ledger storage failure: {source}"),
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
            ImportError::Unbalanced {
                voucher,
                posting_count,
                minor_sum,
            } => {
                write!(
                    f,
                    "voucher {voucher} has {posting_count} postings and is unbalanced by {minor_sum} minor units"
                )
            }
            ImportError::BalanceOverflow { voucher } => {
                write!(f, "voucher {voucher} balance overflows minor units")
            }
            ImportError::IdCountOverflow { row_kind, count } => {
                write!(f, "{row_kind} row count {count} cannot fit in i64")
            }
            ImportError::IdAllocation { source } => {
                write!(f, "id allocation failed: {source}")
            }
        }
    }
}

impl std::error::Error for ImportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ImportError::Store { source } => Some(source.as_ref()),
            ImportError::IdAllocation { source } => Some(source),
            _ => None,
        }
    }
}

impl From<crate::StoreError> for ImportError {
    fn from(source: crate::StoreError) -> ImportError {
        ImportError::Store {
            source: Box::new(source),
        }
    }
}

impl From<IdAllocationError> for ImportError {
    fn from(source: IdAllocationError) -> ImportError {
        ImportError::IdAllocation { source }
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
    let voucher_ids = draft_set.alloc_voucher_ids(voucher_count)?;
    let posting_ids = draft_set.alloc_posting_ids(posting_count)?;

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
        postings.push(Posting {
            id: canonical_id,
            source_id: Some(source.source_id),
            voucher_id,
            account: source.account,
            amount: source.amount,
            text: source.text,
        });
    }

    let violations =
        voucher_balance_violations(&vouchers, &postings).map_err(import_balance_error)?;
    if let Some(violation) = violations.first() {
        return Err(ImportError::Unbalanced {
            voucher: violation.voucher_id,
            posting_count: violation.posting_count,
            minor_sum: violation.minor_sum,
        });
    }

    write_imported_year(&mut draft_set, year, &accounts, &vouchers, &postings)?;
    *set = draft_set;
    Ok(())
}

fn import_balance_error(error: BalanceError) -> ImportError {
    match error {
        BalanceError::SumOverflow { voucher_id } => ImportError::BalanceOverflow {
            voucher: voucher_id,
        },
    }
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
    let store = set.create_year_store(year)?;
    write_rows(&store, set, year, accounts, vouchers, postings)
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
