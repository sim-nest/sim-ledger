//! Ledger-set manifests and set-level id allocation.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::{error, fmt};

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

/// Failure while reserving canonical ids from a ledger-set manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdAllocationError {
    /// The caller requested a negative number of ids.
    NegativeCount {
        /// Row family being allocated.
        row_kind: &'static str,
        /// Requested count.
        count: i64,
    },
    /// The manifest cursor is negative and cannot produce canonical ids.
    NegativeCursor {
        /// Row family being allocated.
        row_kind: &'static str,
        /// Current manifest cursor.
        start: i64,
    },
    /// The requested range would overflow `i64`.
    CursorOverflow {
        /// Row family being allocated.
        row_kind: &'static str,
        /// Current manifest cursor.
        start: i64,
        /// Requested count.
        count: i64,
    },
}

impl fmt::Display for IdAllocationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IdAllocationError::NegativeCount { row_kind, count } => {
                write!(f, "{row_kind} id reservation count {count} is negative")
            }
            IdAllocationError::NegativeCursor { row_kind, start } => {
                write!(f, "{row_kind} id cursor {start} is negative")
            }
            IdAllocationError::CursorOverflow {
                row_kind,
                start,
                count,
            } => {
                write!(
                    f,
                    "{row_kind} id range starting at {start} with count {count} overflows i64"
                )
            }
        }
    }
}

impl error::Error for IdAllocationError {}

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
    pub fn alloc_voucher_ids(&mut self, n: i64) -> Result<Range<i64>, IdAllocationError> {
        reserve_ids("voucher", &mut self.manifest.next_voucher_id, n)
    }

    /// Reserve `n` posting ids and advance the set cursor.
    pub fn alloc_posting_ids(&mut self, n: i64) -> Result<Range<i64>, IdAllocationError> {
        reserve_ids("posting", &mut self.manifest.next_posting_id, n)
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

fn reserve_ids(
    row_kind: &'static str,
    cursor: &mut i64,
    n: i64,
) -> Result<Range<i64>, IdAllocationError> {
    if n < 0 {
        return Err(IdAllocationError::NegativeCount { row_kind, count: n });
    }
    let start = *cursor;
    if start < 0 {
        return Err(IdAllocationError::NegativeCursor { row_kind, start });
    }
    let end = start
        .checked_add(n)
        .ok_or(IdAllocationError::CursorOverflow {
            row_kind,
            start,
            count: n,
        })?;
    *cursor = end;
    Ok(start..end)
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

        assert_eq!(set.alloc_voucher_ids(2).unwrap(), 1..3);
        assert_eq!(set.alloc_posting_ids(3).unwrap(), 1..4);
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

    #[test]
    fn allocators_reject_negative_counts_without_mutating_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let mut set = LedgerSet::create(dir.path(), "Household").unwrap();
        let before = set.manifest.clone();

        let err = set.alloc_voucher_ids(-1).unwrap_err();

        assert_eq!(
            err,
            IdAllocationError::NegativeCount {
                row_kind: "voucher",
                count: -1,
            }
        );
        assert_eq!(set.manifest, before);
    }

    #[test]
    fn allocators_reject_negative_cursors_without_mutating_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let mut set = LedgerSet::create(dir.path(), "Household").unwrap();
        set.manifest.next_voucher_id = -4;
        let before = set.manifest.clone();

        let err = set.alloc_voucher_ids(1).unwrap_err();

        assert_eq!(
            err,
            IdAllocationError::NegativeCursor {
                row_kind: "voucher",
                start: -4,
            }
        );
        assert_eq!(set.manifest, before);
    }

    #[test]
    fn allocators_reject_overflow_without_mutating_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let mut set = LedgerSet::create(dir.path(), "Household").unwrap();
        set.manifest.next_posting_id = i64::MAX;
        let before = set.manifest.clone();

        let err = set.alloc_posting_ids(1).unwrap_err();

        assert_eq!(
            err,
            IdAllocationError::CursorOverflow {
                row_kind: "posting",
                start: i64::MAX,
                count: 1,
            }
        );
        assert_eq!(set.manifest, before);
    }
}
