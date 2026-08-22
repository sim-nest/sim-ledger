//! Trial-balance rows built from one ledger year.
#![allow(missing_docs)]
use crate::{CloseError, checked_i64};
use sim_ledger::LedgerSet;
use std::collections::BTreeMap;
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
    let mut sums = BTreeMap::<i64, (i128, i128, i128)>::new();
    for posting in store.postings()? {
        let row = sums.entry(posting.account).or_default();
        if posting.amount.0 > 0 {
            row.0 += i128::from(posting.amount.0)
        } else {
            row.1 += -i128::from(posting.amount.0)
        }
        row.2 += i128::from(posting.amount.0)
    }
    store
        .accounts()?
        .into_iter()
        .map(|account| {
            let (debit, credit, closing) = sums.remove(&account.number).unwrap_or_default();
            Ok(TrialBalanceRow {
                year,
                account: account.number,
                sru_plus: account.sru_plus,
                sru_minus: account.sru_minus,
                opening_minor: 0,
                debit_minor: checked_i64(debit, "trial debit")?,
                credit_minor: checked_i64(credit, "trial credit")?,
                closing_minor: checked_i64(closing, "trial closing")?,
            })
        })
        .collect()
}
