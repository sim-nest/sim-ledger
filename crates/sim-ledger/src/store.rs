//! Portable persistence for one ledger year on a supplied Table/Dir mount.
#![allow(missing_docs)]

use crate::model::{
    Account, Posting, Voucher, VoucherBalanceViolation, YearData, voucher_balance_violations,
};
use serde::{Deserialize, Serialize};
use sim_storage_port::{HostDirError, HostDirPort, NeverCancel};
use std::{cell::RefCell, collections::BTreeMap, error::Error, fmt, sync::Arc};

#[derive(Debug)]
pub enum StoreError {
    Mount(HostDirError),
    Malformed(String),
    AlreadyExists,
    Closed,
}
impl fmt::Display for StoreError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mount(e) => write!(out, "ledger mount failure: {e}"),
            Self::Malformed(e) => write!(out, "malformed ledger content: {e}"),
            Self::AlreadyExists => out.write_str("ledger content already exists"),
            Self::Closed => out.write_str("ledger year is closed"),
        }
    }
}
impl Error for StoreError {}
impl From<HostDirError> for StoreError {
    fn from(value: HostDirError) -> Self {
        Self::Mount(value)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PersistedYear {
    data: YearData,
    #[serde(default)]
    meta: BTreeMap<String, String>,
    #[serde(default)]
    id_state: BTreeMap<String, i64>,
}

pub struct YearStore {
    mount: Arc<dyn HostDirPort>,
    leaf: String,
    persisted: RefCell<PersistedYear>,
    pub year: i32,
}
impl YearStore {
    pub fn create(mount: Arc<dyn HostDirPort>, year: i32) -> Result<Self, StoreError> {
        let leaf = year_leaf(year);
        if mount.metadata(std::slice::from_ref(&leaf))?.is_some() {
            return Err(StoreError::AlreadyExists);
        }
        let store = Self {
            mount,
            leaf,
            year,
            persisted: RefCell::new(PersistedYear {
                data: YearData {
                    year,
                    ..YearData::default()
                },
                meta: BTreeMap::new(),
                id_state: BTreeMap::new(),
            }),
        };
        store.persist()?;
        Ok(store)
    }
    pub fn open(mount: Arc<dyn HostDirPort>, year: i32) -> Result<Self, StoreError> {
        let leaf = year_leaf(year);
        let bytes = mount.read(std::slice::from_ref(&leaf))?;
        let text = std::str::from_utf8(&bytes).map_err(|e| StoreError::Malformed(e.to_string()))?;
        let persisted: PersistedYear =
            toml::from_str(text).map_err(|e| StoreError::Malformed(e.to_string()))?;
        if persisted.data.year != year {
            return Err(StoreError::Malformed("year identity mismatch".into()));
        }
        Ok(Self {
            mount,
            leaf,
            persisted: RefCell::new(persisted),
            year,
        })
    }
    pub fn accounts(&self) -> Result<Vec<Account>, StoreError> {
        Ok(self.persisted.borrow().data.accounts.clone())
    }
    pub fn vouchers(&self) -> Result<Vec<Voucher>, StoreError> {
        let mut v = self.persisted.borrow().data.vouchers.clone();
        v.sort_by_key(|r| r.id);
        Ok(v)
    }
    pub fn postings(&self) -> Result<Vec<Posting>, StoreError> {
        let mut v = self.persisted.borrow().data.postings.clone();
        v.sort_by_key(|r| r.id);
        Ok(v)
    }
    pub fn insert_account(&self, row: &Account) -> Result<(), StoreError> {
        self.mutate(|p| p.data.accounts.push(row.clone()))
    }
    pub fn insert_voucher(&self, row: &Voucher) -> Result<(), StoreError> {
        self.mutate(|p| p.data.vouchers.push(row.clone()))
    }
    pub fn insert_posting(&self, row: &Posting) -> Result<(), StoreError> {
        self.mutate(|p| p.data.postings.push(row.clone()))
    }
    pub fn set_id_state(&self, kind: &str, next: i64) -> Result<(), StoreError> {
        self.mutate(|p| {
            p.id_state.insert(kind.into(), next);
        })
    }
    pub fn set_meta(&self, key: &str, value: &str) -> Result<(), StoreError> {
        self.mutate_permitted(true, |p| {
            p.meta.insert(key.into(), value.into());
        })
    }
    pub fn meta_value(&self, key: &str) -> Result<Option<String>, StoreError> {
        Ok(self.persisted.borrow().meta.get(key).cloned())
    }
    pub fn is_closed(&self) -> Result<bool, StoreError> {
        Ok(self.meta_value("closing_state")?.as_deref() == Some("closed"))
    }
    pub fn voucher_balance_violations(&self) -> Result<Vec<VoucherBalanceViolation>, StoreError> {
        voucher_balance_violations(&self.vouchers()?, &self.postings()?)
            .map_err(|e| StoreError::Malformed(e.to_string()))
    }
    fn mutate(&self, edit: impl FnOnce(&mut PersistedYear)) -> Result<(), StoreError> {
        self.mutate_permitted(false, edit)
    }
    fn mutate_permitted(
        &self,
        permit_closed: bool,
        edit: impl FnOnce(&mut PersistedYear),
    ) -> Result<(), StoreError> {
        if !permit_closed && self.is_closed()? {
            return Err(StoreError::Closed);
        }
        edit(&mut self.persisted.borrow_mut());
        self.persist()
    }
    fn persist(&self) -> Result<(), StoreError> {
        let text = toml::to_string(&*self.persisted.borrow())
            .map_err(|e| StoreError::Malformed(e.to_string()))?;
        self.mount.replace(
            std::slice::from_ref(&self.leaf),
            text.as_bytes(),
            &NeverCancel,
        )?;
        Ok(())
    }
}
fn year_leaf(year: i32) -> String {
    format!("year-{year}.toml")
}
