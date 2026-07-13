//! Trial-balance rows built from one ledger year.

use sim_ledger::{LedgerSet, YearStore};

use crate::CloseError;

/// Trial-balance row for one year-local account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrialBalanceRow {
    /// Ledger year.
    pub year: i32,
    /// Year-local account number.
    pub account: i64,
    /// SRU code used for positive balances.
    pub sru_plus: Option<i32>,
    /// SRU code used for negative balances.
    pub sru_minus: Option<i32>,
    /// Opening balance in exact minor units.
    pub opening_minor: i64,
    /// Debit movement in exact minor units.
    pub debit_minor: i64,
    /// Credit movement as a positive exact minor-unit magnitude.
    pub credit_minor: i64,
    /// Closing balance in exact signed minor units.
    pub closing_minor: i64,
}

impl TrialBalanceRow {
    /// Returns the SRU code that applies to this row's closing balance.
    #[must_use]
    pub fn closing_sru(&self) -> Option<i32> {
        if self.closing_minor >= 0 {
            self.sru_plus.or(self.sru_minus)
        } else {
            self.sru_minus.or(self.sru_plus)
        }
    }
}

/// Build the trial balance for one year.
pub fn trial_balance(set: &LedgerSet, year: i32) -> Result<Vec<TrialBalanceRow>, CloseError> {
    let store = YearStore::open(&set.year_path(year))?;
    let mut stmt = store.conn.prepare(
        "SELECT a.number, a.sru_plus, a.sru_minus, \
         COALESCE(SUM(CASE WHEN p.minor > 0 THEN p.minor ELSE 0 END), 0), \
         COALESCE(SUM(CASE WHEN p.minor < 0 THEN -p.minor ELSE 0 END), 0), \
         COALESCE(SUM(p.minor), 0) \
         FROM account a \
         LEFT JOIN posting p ON p.account = a.number \
         GROUP BY a.number, a.sru_plus, a.sru_minus \
         ORDER BY a.number",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(TrialBalanceRow {
            year,
            account: row.get(0)?,
            sru_plus: row.get(1)?,
            sru_minus: row.get(2)?,
            opening_minor: 0,
            debit_minor: row.get(3)?,
            credit_minor: row.get(4)?,
            closing_minor: row.get(5)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(CloseError::from)
}
