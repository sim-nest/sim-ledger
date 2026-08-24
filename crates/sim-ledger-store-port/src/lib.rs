//! Provider-neutral relational year-file contract for ledger persistence.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use sim_relation_site::{Session, SiteError};
use sim_storage_port::{HostDirError, HostDirPort};
use std::{fmt, sync::Arc};

/// Stable ledger persistence failure categories.
#[derive(Debug)]
pub enum StoreError {
    /// Supplied mount failed.
    Mount(HostDirError),
    /// Caller, schema, or adapter value was invalid.
    Invalid(String),
    /// Persisted ledger content was malformed.
    Malformed(String),
    /// Relational provider failed.
    Storage(SiteError),
    /// Exclusive creation found an existing year or set file.
    AlreadyExists,
    /// Accounting policy refused mutation of a closed year.
    Closed,
}
impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mount(error) => write!(f, "ledger mount failure: {error}"),
            Self::Invalid(message) => write!(f, "invalid ledger operation: {message}"),
            Self::Malformed(message) => write!(f, "malformed ledger content: {message}"),
            Self::Storage(error) => write!(f, "ledger relation failure: {error}"),
            Self::AlreadyExists => f.write_str("ledger content already exists"),
            Self::Closed => f.write_str("ledger year is closed"),
        }
    }
}
impl std::error::Error for StoreError {}
impl From<HostDirError> for StoreError {
    fn from(v: HostDirError) -> Self {
        Self::Mount(v)
    }
}
impl From<SiteError> for StoreError {
    fn from(v: SiteError) -> Self {
        Self::Storage(v)
    }
}

/// Private relation file supplied by a platform adapter.
pub trait RelationYearFile {
    /// Returns the adapter-owned session for one product operation.
    fn session(&mut self) -> &mut dyn Session;
    /// Atomically materializes committed provider state through the supplied mount.
    fn persist(&mut self) -> Result<(), StoreError>;
}

/// Injected owner of provider placement and exact file materialization.
pub trait YearFileFactory: Send + Sync {
    /// Exclusively creates a year file from the canonical empty image.
    fn create(
        &self,
        mount: Arc<dyn HostDirPort>,
        leaf: &str,
        initial: &[u8],
    ) -> Result<Box<dyn RelationYearFile>, StoreError>;
    /// Opens an exact existing year file.
    fn open(
        &self,
        mount: Arc<dyn HostDirPort>,
        leaf: &str,
    ) -> Result<Box<dyn RelationYearFile>, StoreError>;
}
