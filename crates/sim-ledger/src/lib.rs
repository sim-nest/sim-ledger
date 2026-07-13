//! Yearly ledger records and exact amount helpers.
//!
//! The crate keeps the shared ledger model small: accounts belong to one year,
//! vouchers group posting lines, and amounts are stored as signed hundredths so
//! sums are exact.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod model;
pub mod store;

pub use model::{Account, Amount, Posting, Voucher, YearData, is_balanced};
pub use store::YearStore;
