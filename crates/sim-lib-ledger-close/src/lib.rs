//! Fiscal-year close and financial statement helpers.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod adjustments;
pub mod lock;
pub mod period;
pub mod statements;
pub mod trial_balance;

use std::fmt;

pub use adjustments::StatementAdjustment;
pub use lock::{CloseJournalEntry, close_journal, close_state, close_year, reopen_year};
pub use period::ClosingState;
pub use statements::{
    FinancialStatements, SruComparativeRow, SruYearAmount, StatementNote, StatementRow,
    StatementTable, compare_by_sru, financial_statements,
};
pub use trial_balance::{TrialBalanceRow, trial_balance};

/// Failure while closing or projecting a ledger year.
#[derive(Debug)]
pub enum CloseError {
    /// Filesystem work failed.
    Io(std::io::Error),
    /// SQLite work failed.
    Sql(rusqlite::Error),
    /// Trial balance postings do not net to zero.
    UnbalancedTrialBalance {
        /// Signed minor-unit sum across all trial-balance rows.
        minor_sum: i64,
    },
    /// A close/reopen state entry was malformed.
    InvalidState(String),
    /// An exact minor-unit calculation overflowed.
    ArithmeticOverflow(&'static str),
}

impl fmt::Display for CloseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "ledger close I/O failed: {error}"),
            Self::Sql(error) => write!(f, "ledger close SQL failed: {error}"),
            Self::UnbalancedTrialBalance { minor_sum } => {
                write!(f, "trial balance is unbalanced by {minor_sum} minor units")
            }
            Self::InvalidState(message) => write!(f, "invalid close state: {message}"),
            Self::ArithmeticOverflow(role) => write!(f, "{role} overflows minor units"),
        }
    }
}

impl std::error::Error for CloseError {}

impl From<std::io::Error> for CloseError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<rusqlite::Error> for CloseError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sql(error)
    }
}

pub(crate) fn checked_i64(sum: i128, role: &'static str) -> Result<i64, CloseError> {
    i64::try_from(sum).map_err(|_| CloseError::ArithmeticOverflow(role))
}

#[cfg(test)]
mod tests;
