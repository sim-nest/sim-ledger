//! Trial-balance rows built from one ledger year.
#![allow(missing_docs)]
use crate::CloseError;
use sim_ledger::LedgerSet;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrialBalanceRow {
    pub year: i32,
    pub account: i64,
    pub sru_plus: Option<i32>,
    pub sru_minus: Option<i32>,
    pub opening_minor: i64,
    pub debit_minor: i64,
    pub credit_minor: i64,
    pub closing_minor: i64,
}
impl TrialBalanceRow {
    #[must_use]
    pub fn closing_sru(&self) -> Option<i32> {
        if self.closing_minor >= 0 {
            self.sru_plus.or(self.sru_minus)
        } else {
            self.sru_minus.or(self.sru_plus)
        }
    }
}
pub fn trial_balance(set: &LedgerSet, year: i32) -> Result<Vec<TrialBalanceRow>, CloseError> {
    let store = set.year_store(year)?;
    store
        .trial_balance_data()?
        .into_iter()
        .map(|row| {
            Ok(TrialBalanceRow {
                year,
                account: row.account,
                sru_plus: row.sru_plus,
                sru_minus: row.sru_minus,
                opening_minor: 0,
                debit_minor: row.debit_minor,
                credit_minor: row.credit_minor,
                closing_minor: row.closing_minor,
            })
        })
        .collect()
}
