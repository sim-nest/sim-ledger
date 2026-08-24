//! Ledger-set manifests and set-level id allocation on supplied mounts.
#![allow(missing_docs)]

use crate::store::{StoreError, YearFileFactory, YearStore};
use sim_storage_port::{HostDirPort, NeverCancel};
use std::{error, fmt, ops::Range, sync::Arc};
const MANIFEST_FILE: &str = "ledger-set.toml";

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct SetManifest {
    pub label: String,
    pub next_voucher_id: i64,
    pub next_posting_id: i64,
    pub years: Vec<i32>,
}
#[derive(Clone)]
pub struct LedgerSet {
    mount: Arc<dyn HostDirPort>,
    year_files: Arc<dyn YearFileFactory>,
    pub manifest: SetManifest,
}
impl fmt::Debug for LedgerSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LedgerSet")
            .field("mount", &self.mount.label())
            .field("manifest", &self.manifest)
            .finish()
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdAllocationError {
    NegativeCount {
        row_kind: &'static str,
        count: i64,
    },
    NegativeCursor {
        row_kind: &'static str,
        start: i64,
    },
    CursorOverflow {
        row_kind: &'static str,
        start: i64,
        count: i64,
    },
}
impl fmt::Display for IdAllocationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl error::Error for IdAllocationError {}
impl LedgerSet {
    pub fn create(
        mount: Arc<dyn HostDirPort>,
        year_files: Arc<dyn YearFileFactory>,
        label: &str,
    ) -> Result<Self, StoreError> {
        if mount.metadata(&[MANIFEST_FILE.into()])?.is_some() {
            return Err(StoreError::AlreadyExists);
        }
        let set = Self {
            mount,
            year_files,
            manifest: SetManifest {
                label: label.into(),
                next_voucher_id: 1,
                next_posting_id: 1,
                years: vec![],
            },
        };
        set.save()?;
        Ok(set)
    }
    pub fn open(
        mount: Arc<dyn HostDirPort>,
        year_files: Arc<dyn YearFileFactory>,
    ) -> Result<Self, StoreError> {
        let bytes = mount.read(&[MANIFEST_FILE.into()])?;
        let text = std::str::from_utf8(&bytes).map_err(|e| StoreError::Malformed(e.to_string()))?;
        let manifest = toml::from_str(text).map_err(|e| StoreError::Malformed(e.to_string()))?;
        Ok(Self {
            mount,
            year_files,
            manifest,
        })
    }
    pub fn save(&self) -> Result<(), StoreError> {
        let text = toml::to_string_pretty(&self.manifest)
            .map_err(|e| StoreError::Malformed(e.to_string()))?;
        self.mount
            .replace(&[MANIFEST_FILE.into()], text.as_bytes(), &NeverCancel)?;
        Ok(())
    }
    pub fn year_store(&self, year: i32) -> Result<YearStore, StoreError> {
        YearStore::open(self.year_files.as_ref(), self.mount.clone(), year)
    }
    pub fn create_year_store(&self, year: i32) -> Result<YearStore, StoreError> {
        YearStore::create(self.year_files.as_ref(), self.mount.clone(), year)
    }
    pub fn mount(&self) -> Arc<dyn HostDirPort> {
        self.mount.clone()
    }
    pub fn alloc_voucher_ids(&mut self, n: i64) -> Result<Range<i64>, IdAllocationError> {
        reserve_ids("voucher", &mut self.manifest.next_voucher_id, n)
    }
    pub fn alloc_posting_ids(&mut self, n: i64) -> Result<Range<i64>, IdAllocationError> {
        reserve_ids("posting", &mut self.manifest.next_posting_id, n)
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
