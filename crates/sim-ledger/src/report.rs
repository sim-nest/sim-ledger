//! Cross-year ledger reports over read-only attached year files.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, params};

use crate::model::Amount;
use crate::set::LedgerSet;

/// Grouping key for a balance report row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BalanceKey {
    /// Balance for one year-local account.
    Account {
        /// Ledger year that owns the account.
        year: i32,
        /// Year-local account number.
        account: i64,
    },
    /// Balance grouped by an SRU code across years and account numbers.
    Sru {
        /// SRU code selected from each year-local account.
        code: i32,
    },
}

/// One balance report row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BalanceRow {
    /// Report grouping key.
    pub key: BalanceKey,
    /// Exact signed balance for the key.
    pub amount: Amount,
}

/// Sum postings across selected years.
///
/// With `by_sru = false`, rows are grouped by `(year, account)` so year-local
/// account numbers stay distinct. With `by_sru = true`, rows are grouped by
/// each account's SRU code, so different account numbers can roll up together.
pub fn balances(set: &LedgerSet, years: &[i32], by_sru: bool) -> rusqlite::Result<Vec<BalanceRow>> {
    if years.is_empty() {
        return Ok(Vec::new());
    }

    let conn = Connection::open_in_memory_with_flags(
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_URI,
    )?;
    let attached = attach_years(&conn, set, years)?;

    if by_sru {
        sru_balances(&conn, &attached)
    } else {
        account_balances(&conn, &attached)
    }
}

struct AttachedYear {
    year: i32,
    alias: String,
}

fn attach_years(
    conn: &Connection,
    set: &LedgerSet,
    years: &[i32],
) -> rusqlite::Result<Vec<AttachedYear>> {
    let mut attached = Vec::with_capacity(years.len());
    for (index, year) in years.iter().copied().enumerate() {
        let alias = format!("y{index}");
        let uri = sqlite_file_uri(&set.year_path(year))?;
        conn.execute(&format!("ATTACH DATABASE ?1 AS {alias}"), params![uri])?;
        attached.push(AttachedYear { year, alias });
    }
    Ok(attached)
}

fn account_balances(
    conn: &Connection,
    attached: &[AttachedYear],
) -> rusqlite::Result<Vec<BalanceRow>> {
    let mut sql = String::from("SELECT year, account, SUM(minor) FROM (");
    for (index, year) in attached.iter().enumerate() {
        if index > 0 {
            sql.push_str(" UNION ALL ");
        }
        write!(
            sql,
            "SELECT {} AS year, account, minor FROM {}.posting",
            year.year, year.alias
        )
        .expect("writing SQL to a string cannot fail");
    }
    sql.push_str(") GROUP BY year, account ORDER BY year, account");

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok(BalanceRow {
            key: BalanceKey::Account {
                year: row.get(0)?,
                account: row.get(1)?,
            },
            amount: Amount(row.get(2)?),
        })
    })?;
    rows.collect()
}

fn sru_balances(conn: &Connection, attached: &[AttachedYear]) -> rusqlite::Result<Vec<BalanceRow>> {
    let mut sql = String::from("SELECT sru, SUM(minor) FROM (");
    for (index, year) in attached.iter().enumerate() {
        if index > 0 {
            sql.push_str(" UNION ALL ");
        }
        write!(
            sql,
            "SELECT COALESCE(\
             CASE WHEN p.minor >= 0 THEN a.sru_plus ELSE a.sru_minus END, \
             a.sru_plus, \
             a.sru_minus\
             ) AS sru, p.minor AS minor \
             FROM {}.posting p JOIN {}.account a ON a.number = p.account",
            year.alias, year.alias
        )
        .expect("writing SQL to a string cannot fail");
    }
    sql.push_str(") WHERE sru IS NOT NULL GROUP BY sru ORDER BY sru");

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok(BalanceRow {
            key: BalanceKey::Sru { code: row.get(0)? },
            amount: Amount(row.get(1)?),
        })
    })?;
    rows.collect()
}

fn sqlite_file_uri(path: &Path) -> rusqlite::Result<String> {
    let absolute = absolute_path(path)?;
    let path_text = absolute
        .to_str()
        .ok_or_else(|| rusqlite::Error::InvalidPath(path.to_path_buf()))?;
    Ok(format!("file:{}?mode=ro", percent_encode_path(path_text)))
}

fn absolute_path(path: &Path) -> rusqlite::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        let cwd = std::env::current_dir()
            .map_err(|_| rusqlite::Error::InvalidPath(path.to_path_buf()))?;
        Ok(cwd.join(path))
    }
}

fn percent_encode_path(path: &str) -> String {
    let mut encoded = String::new();
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            encoded.push(byte as char);
        } else {
            write!(encoded, "%{byte:02X}").expect("writing URI text to a string cannot fail");
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::model::{Account, Posting, Voucher};
    use crate::store::YearStore;

    #[test]
    fn set_allocators_feed_cross_year_reports() {
        let dir = tempfile::tempdir().unwrap();
        let mut set = LedgerSet::create(dir.path(), "Household").unwrap();
        set.manifest.next_voucher_id = 100;
        set.manifest.next_posting_id = 500;
        set.save().unwrap();

        let first_ids = write_year(&mut set, 2022, 1910, 3010, 1_200);
        let second_ids = write_year(&mut set, 2023, 1930, 3020, 800);
        assert!(first_ids.is_disjoint(&second_ids));

        let reloaded = LedgerSet::open(dir.path()).unwrap();
        assert_eq!(reloaded.manifest.next_voucher_id, 102);
        assert_eq!(reloaded.manifest.next_posting_id, 504);

        assert_eq!(
            balances(&reloaded, &[2022, 2023], false).unwrap(),
            vec![
                BalanceRow {
                    key: BalanceKey::Account {
                        year: 2022,
                        account: 1910
                    },
                    amount: Amount(1_200),
                },
                BalanceRow {
                    key: BalanceKey::Account {
                        year: 2022,
                        account: 3010
                    },
                    amount: Amount(-1_200),
                },
                BalanceRow {
                    key: BalanceKey::Account {
                        year: 2023,
                        account: 1930
                    },
                    amount: Amount(800),
                },
                BalanceRow {
                    key: BalanceKey::Account {
                        year: 2023,
                        account: 3020
                    },
                    amount: Amount(-800),
                },
            ]
        );
        assert_eq!(
            balances(&reloaded, &[2022, 2023], true).unwrap(),
            vec![
                BalanceRow {
                    key: BalanceKey::Sru { code: 1000 },
                    amount: Amount(2_000),
                },
                BalanceRow {
                    key: BalanceKey::Sru { code: 3000 },
                    amount: Amount(-2_000),
                },
            ]
        );
    }

    fn write_year(
        set: &mut LedgerSet,
        year: i32,
        debit_account: i64,
        credit_account: i64,
        minor: i64,
    ) -> BTreeSet<i64> {
        let voucher_id = set.alloc_voucher_ids(1).start;
        let posting_ids: Vec<i64> = set.alloc_posting_ids(2).collect();
        let store = YearStore::create(&set.year_path(year), year).unwrap();
        store
            .insert_account(&account(debit_account, "Asset", Some(1000), None))
            .unwrap();
        store
            .insert_account(&account(credit_account, "Income", None, Some(3000)))
            .unwrap();
        store
            .insert_voucher(&Voucher {
                id: voucher_id,
                source_id: Some(i64::from(year)),
                date: format!("{year}-01-31"),
                text: Some("Monthly result".to_owned()),
            })
            .unwrap();
        store
            .insert_posting(&Posting {
                id: posting_ids[0],
                source_id: Some(posting_ids[0] + 10_000),
                voucher_id,
                account: debit_account,
                amount: Amount(minor),
                text: Some("Debit".to_owned()),
            })
            .unwrap();
        store
            .insert_posting(&Posting {
                id: posting_ids[1],
                source_id: Some(posting_ids[1] + 10_000),
                voucher_id,
                account: credit_account,
                amount: Amount(-minor),
                text: Some("Credit".to_owned()),
            })
            .unwrap();
        store
            .set_id_state("voucher", set.manifest.next_voucher_id)
            .unwrap();
        store
            .set_id_state("posting", set.manifest.next_posting_id)
            .unwrap();

        set.manifest.years.push(year);
        set.save().unwrap();
        let reopened = YearStore::open(&set.year_path(year)).unwrap();
        reopened
            .postings()
            .unwrap()
            .into_iter()
            .map(|p| p.id)
            .collect()
    }

    fn account(number: i64, name: &str, sru_plus: Option<i32>, sru_minus: Option<i32>) -> Account {
        Account {
            number,
            name: name.to_owned(),
            note: None,
            sru_plus,
            sru_minus,
        }
    }
}
