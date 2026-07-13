//! Ledger-set manifests and set-level id allocation.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};

const MANIFEST_FILE: &str = "ledger-set.toml";
const YEARS_DIR: &str = "years";

/// Manifest stored at the root of a ledger set.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct SetManifest {
    /// Human-readable label for the set.
    pub label: String,
    /// Authoritative next voucher id for cross-year carry-over.
    pub next_voucher_id: i64,
    /// Authoritative next posting id for cross-year carry-over.
    pub next_posting_id: i64,
    /// Years currently present in the set.
    pub years: Vec<i32>,
}

/// A directory that owns a ledger-set manifest and per-year SQLite files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerSet {
    /// Root directory for the ledger set.
    pub dir: PathBuf,
    /// Loaded manifest for the set.
    pub manifest: SetManifest,
}

impl LedgerSet {
    /// Create a new ledger set directory with an empty manifest.
    pub fn create(dir: &Path, label: &str) -> io::Result<LedgerSet> {
        fs::create_dir_all(dir.join(YEARS_DIR))?;
        let set = LedgerSet {
            dir: dir.to_path_buf(),
            manifest: SetManifest {
                label: label.to_owned(),
                next_voucher_id: 1,
                next_posting_id: 1,
                years: Vec::new(),
            },
        };
        set.write_manifest(true)?;
        Ok(set)
    }

    /// Open an existing ledger set directory.
    pub fn open(dir: &Path) -> io::Result<LedgerSet> {
        let text = fs::read_to_string(dir.join(MANIFEST_FILE))?;
        let manifest = toml::from_str(&text).map_err(invalid_manifest)?;
        Ok(LedgerSet {
            dir: dir.to_path_buf(),
            manifest,
        })
    }

    /// Persist the current manifest to `ledger-set.toml`.
    pub fn save(&self) -> io::Result<()> {
        fs::create_dir_all(self.dir.join(YEARS_DIR))?;
        self.write_manifest(false)
    }

    /// Return the SQLite path for one year in this set.
    #[must_use]
    pub fn year_path(&self, year: i32) -> PathBuf {
        self.dir.join(YEARS_DIR).join(format!("{year}.sqlite"))
    }

    /// Reserve `n` voucher ids and advance the set cursor.
    #[must_use]
    pub fn alloc_voucher_ids(&mut self, n: i64) -> Range<i64> {
        reserve_ids(&mut self.manifest.next_voucher_id, n)
    }

    /// Reserve `n` posting ids and advance the set cursor.
    #[must_use]
    pub fn alloc_posting_ids(&mut self, n: i64) -> Range<i64> {
        reserve_ids(&mut self.manifest.next_posting_id, n)
    }

    fn write_manifest(&self, create_new: bool) -> io::Result<()> {
        let text = toml::to_string_pretty(&self.manifest).map_err(invalid_manifest)?;
        let path = self.dir.join(MANIFEST_FILE);
        if create_new {
            let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
            file.write_all(text.as_bytes())
        } else {
            fs::write(path, text)
        }
    }
}

fn reserve_ids(cursor: &mut i64, n: i64) -> Range<i64> {
    assert!(n >= 0, "id reservation count must be non-negative");
    let start = *cursor;
    let end = start
        .checked_add(n)
        .expect("id reservation must not overflow i64");
    *cursor = end;
    start..end
}

fn invalid_manifest<E>(err: E) -> io::Error
where
    E: std::error::Error + Send + Sync + 'static,
{
    io::Error::new(io::ErrorKind::InvalidData, err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_persists_and_allocators_advance() {
        let dir = tempfile::tempdir().unwrap();
        let mut set = LedgerSet::create(dir.path(), "Household").unwrap();

        assert_eq!(set.alloc_voucher_ids(2), 1..3);
        assert_eq!(set.alloc_posting_ids(3), 1..4);
        set.manifest.years.push(2022);
        set.save().unwrap();

        let reloaded = LedgerSet::open(dir.path()).unwrap();
        assert_eq!(reloaded.manifest.label, "Household");
        assert_eq!(reloaded.manifest.next_voucher_id, 3);
        assert_eq!(reloaded.manifest.next_posting_id, 4);
        assert_eq!(reloaded.manifest.years, vec![2022]);
        assert_eq!(
            reloaded.year_path(2022),
            dir.path().join("years/2022.sqlite")
        );
    }
}
