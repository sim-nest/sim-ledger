//! SQLite storage for one immutable ledger year file.

use std::fs::OpenOptions;
use std::path::Path;

use rusqlite::types::Type;
use rusqlite::{Connection, params};

use crate::model::{Account, Amount, Posting, Voucher};

const SCHEMA: &str = include_str!("schema.sql");

/// A connection to one per-year SQLite ledger file.
pub struct YearStore {
    /// Open SQLite connection for the year file.
    pub conn: Connection,
    /// Ledger year carried by this file.
    pub year: i32,
}

impl YearStore {
    /// Create a fresh `<year>.sqlite` with the ledger schema.
    ///
    /// The call uses SQLite exclusive creation, so it fails when `path` already
    /// exists.
    pub fn create(path: &Path, year: i32) -> rusqlite::Result<YearStore> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|_| rusqlite::Error::InvalidPath(path.to_path_buf()))?;
        drop(file);
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        let store = YearStore { conn, year };
        store.set_meta("year", &year.to_string())?;
        Ok(store)
    }

    /// Open an existing year file.
    pub fn open(path: &Path) -> rusqlite::Result<YearStore> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let year = conn.query_row("SELECT value FROM meta WHERE key = 'year'", [], |row| {
            let value: String = row.get(0)?;
            parse_year(value)
        })?;
        Ok(YearStore { conn, year })
    }

    /// Insert one year-local account.
    pub fn insert_account(&self, account: &Account) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO account(number, name, note, sru_plus, sru_minus) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                account.number,
                account.name,
                account.note,
                account.sru_plus,
                account.sru_minus
            ],
        )?;
        Ok(())
    }

    /// Insert one voucher.
    pub fn insert_voucher(&self, voucher: &Voucher) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO voucher(id, source_id, date, text) VALUES (?1, ?2, ?3, ?4)",
            params![voucher.id, voucher.source_id, voucher.date, voucher.text],
        )?;
        Ok(())
    }

    /// Insert one posting line.
    pub fn insert_posting(&self, posting: &Posting) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO posting(id, source_id, voucher_id, account, minor, text) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                posting.id,
                posting.source_id,
                posting.voucher_id,
                posting.account,
                posting.amount.0,
                posting.text
            ],
        )?;
        Ok(())
    }

    /// Store a mirrored id cursor for a closed year file.
    pub fn set_id_state(&self, kind: &str, next: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO id_state(kind, next) VALUES (?1, ?2) \
             ON CONFLICT(kind) DO UPDATE SET next = excluded.next",
            params![kind, next],
        )?;
        Ok(())
    }

    /// Store one self-describing metadata entry.
    pub fn set_meta(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO meta(key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Read all vouchers ordered by canonical id.
    pub fn vouchers(&self) -> rusqlite::Result<Vec<Voucher>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, source_id, date, text FROM voucher ORDER BY id")?;
        let rows = stmt.query_map([], |row| {
            Ok(Voucher {
                id: row.get(0)?,
                source_id: row.get(1)?,
                date: row.get(2)?,
                text: row.get(3)?,
            })
        })?;
        rows.collect()
    }

    /// Read all postings ordered by canonical id.
    pub fn postings(&self) -> rusqlite::Result<Vec<Posting>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, source_id, voucher_id, account, minor, text FROM posting ORDER BY id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Posting {
                id: row.get(0)?,
                source_id: row.get(1)?,
                voucher_id: row.get(2)?,
                account: row.get(3)?,
                amount: Amount(row.get(4)?),
                text: row.get(5)?,
            })
        })?;
        rows.collect()
    }
}

fn parse_year(value: String) -> rusqlite::Result<i32> {
    value
        .parse::<i32>()
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(err)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn year_store_round_trips_balanced_voucher() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("2022.sqlite");
        {
            let store = YearStore::create(&path, 2022).unwrap();
            store.insert_account(&account(1910, "Cash")).unwrap();
            store.insert_account(&account(3010, "Sales")).unwrap();
            store
                .insert_voucher(&Voucher {
                    id: 100,
                    source_id: Some(17),
                    date: "2022-05-04".to_owned(),
                    text: Some("Receipt".to_owned()),
                })
                .unwrap();
            store
                .insert_posting(&Posting {
                    id: 200,
                    source_id: Some(31),
                    voucher_id: 100,
                    account: 1910,
                    amount: Amount(12_345),
                    text: Some("Bank".to_owned()),
                })
                .unwrap();
            store
                .insert_posting(&Posting {
                    id: 201,
                    source_id: Some(32),
                    voucher_id: 100,
                    account: 3010,
                    amount: Amount(-12_345),
                    text: Some("Revenue".to_owned()),
                })
                .unwrap();
            store.set_id_state("voucher", 101).unwrap();
            store.set_id_state("posting", 202).unwrap();
        }

        let store = YearStore::open(&path).unwrap();
        assert_eq!(store.year, 2022);
        assert_eq!(
            store.vouchers().unwrap(),
            vec![Voucher {
                id: 100,
                source_id: Some(17),
                date: "2022-05-04".to_owned(),
                text: Some("Receipt".to_owned()),
            }]
        );
        assert_eq!(
            store.postings().unwrap(),
            vec![
                Posting {
                    id: 200,
                    source_id: Some(31),
                    voucher_id: 100,
                    account: 1910,
                    amount: Amount(12_345),
                    text: Some("Bank".to_owned()),
                },
                Posting {
                    id: 201,
                    source_id: Some(32),
                    voucher_id: 100,
                    account: 3010,
                    amount: Amount(-12_345),
                    text: Some("Revenue".to_owned()),
                },
            ]
        );
        let sum: i64 = store
            .conn
            .query_row("SELECT SUM(minor) FROM posting", [], |row| row.get(0))
            .unwrap();
        assert_eq!(sum, 0);
    }

    #[test]
    fn create_fails_when_year_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("2023.sqlite");

        let store = YearStore::create(&path, 2023).unwrap();
        drop(store);

        assert!(YearStore::create(&path, 2023).is_err());
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
}
