//! Yearly ledger records and exact amount helpers.
//!
//! The crate keeps the shared ledger model small: accounts belong to one year,
//! vouchers group posting lines, and amounts are stored as signed hundredths so
//! sums are exact.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

#[cfg(feature = "sim")]
pub mod codec;
pub mod import;
pub mod model;
pub mod report;
pub mod set;
pub mod store;

#[cfg(feature = "sim")]
pub use codec::{
    BalancesCall, LedgerCodecError, balances_call_from_expr, balances_query_expr, report_to_expr,
};
pub use import::{ImportError, SourcePosting, SourceVoucher, SourceYear, import_year};
pub use model::{Account, Amount, Posting, Voucher, YearData, is_balanced};
pub use report::{BalanceKey, BalanceRow, balances};
pub use set::{LedgerSet, SetManifest};
pub use store::YearStore;
