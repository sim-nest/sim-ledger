//! Financial statement tables and SRU comparisons.

use std::collections::{BTreeMap, BTreeSet};

use sim_ledger::{BalanceKey, LedgerSet, YearStore, balances};

use crate::trial_balance::{TrialBalanceRow, trial_balance};
use crate::{CloseError, checked_i64};

/// One statement table row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatementRow {
    /// Row label.
    pub label: String,
    /// Signed exact minor-unit amount.
    pub amount_minor: i64,
}

/// One statement table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatementTable {
    /// Table title.
    pub title: String,
    /// Ordered rows.
    pub rows: Vec<StatementRow>,
}

impl StatementTable {
    /// Sums all row amounts.
    pub fn total_minor(&self) -> Result<i64, CloseError> {
        checked_i64(
            self.rows
                .iter()
                .map(|row| i128::from(row.amount_minor))
                .sum(),
            "statement total",
        )
    }
}

/// One plain-language statement note.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatementNote {
    /// Stable note id.
    pub id: String,
    /// Note text.
    pub text: String,
}

/// Complete statements for one fiscal year.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinancialStatements {
    /// Ledger year.
    pub year: i32,
    /// Trial-balance rows.
    pub trial_balance: Vec<TrialBalanceRow>,
    /// Income statement table.
    pub income_statement: StatementTable,
    /// Balance sheet table.
    pub balance_sheet: StatementTable,
    /// Statement notes.
    pub notes: Vec<StatementNote>,
}

impl FinancialStatements {
    /// Sums the trial-balance closing column.
    pub fn trial_balance_total_minor(&self) -> Result<i64, CloseError> {
        checked_i64(
            self.trial_balance
                .iter()
                .map(|row| i128::from(row.closing_minor))
                .sum(),
            "trial-balance total",
        )
    }
}

/// One year amount in an SRU comparison row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SruYearAmount {
    /// Ledger year.
    pub year: i32,
    /// Signed exact minor-unit amount.
    pub amount_minor: i64,
}

/// One SRU comparison row across selected years.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SruComparativeRow {
    /// SRU code.
    pub sru: i32,
    /// Amounts ordered like the requested years.
    pub years: Vec<SruYearAmount>,
}

/// Build financial statements from one ledger year.
pub fn financial_statements(set: &LedgerSet, year: i32) -> Result<FinancialStatements, CloseError> {
    ensure_vouchers_balanced(set, year)?;
    let trial_balance = trial_balance(set, year)?;
    let total = checked_i64(
        trial_balance
            .iter()
            .map(|row| i128::from(row.closing_minor))
            .sum(),
        "trial-balance total",
    )?;
    if total != 0 {
        return Err(CloseError::UnbalancedTrialBalance { minor_sum: total });
    }

    let income_statement = table_from_rows("Income statement", &trial_balance, true)?;
    let balance_sheet = table_from_rows("Balance sheet", &trial_balance, false)?;
    Ok(FinancialStatements {
        year,
        trial_balance,
        income_statement,
        balance_sheet,
        notes: vec![StatementNote {
            id: "basis".to_owned(),
            text: "Amounts are signed exact minor units from the ledger year.".to_owned(),
        }],
    })
}

/// Compare balances across years by SRU code.
pub fn compare_by_sru(
    set: &LedgerSet,
    years: &[i32],
) -> Result<Vec<SruComparativeRow>, CloseError> {
    let mut by_year = Vec::with_capacity(years.len());
    let mut all_sru = BTreeSet::new();
    for year in years {
        ensure_vouchers_balanced(set, *year)?;
        let mut year_rows = BTreeMap::new();
        for row in balances(set, &[*year], true)? {
            if let BalanceKey::Sru { code } = row.key {
                all_sru.insert(code);
                year_rows.insert(code, row.amount.0);
            }
        }
        by_year.push((*year, year_rows));
    }

    Ok(all_sru
        .into_iter()
        .map(|sru| SruComparativeRow {
            sru,
            years: by_year
                .iter()
                .map(|(year, rows)| SruYearAmount {
                    year: *year,
                    amount_minor: rows.get(&sru).copied().unwrap_or_default(),
                })
                .collect(),
        })
        .collect())
}

fn ensure_vouchers_balanced(set: &LedgerSet, year: i32) -> Result<(), CloseError> {
    let store = YearStore::open(&set.year_path(year))?;
    if let Some(violation) = store.voucher_balance_violations()?.into_iter().next() {
        return Err(CloseError::UnbalancedVoucher {
            voucher: violation.voucher_id,
            posting_count: violation.posting_count,
            minor_sum: violation.minor_sum,
        });
    }
    Ok(())
}

fn table_from_rows(
    title: &str,
    rows: &[TrialBalanceRow],
    income: bool,
) -> Result<StatementTable, CloseError> {
    let mut grouped = BTreeMap::<String, i128>::new();
    for row in rows.iter().filter(|row| row.closing_minor != 0) {
        let label = row
            .closing_sru()
            .map(|sru| format!("SRU {sru}"))
            .unwrap_or_else(|| format!("Account {}", row.account));
        if is_income_sru(row.closing_sru()) == income {
            *grouped.entry(label).or_default() += i128::from(row.closing_minor);
        }
    }
    let rows = grouped
        .into_iter()
        .map(|(label, amount)| {
            Ok(StatementRow {
                label,
                amount_minor: checked_i64(amount, "statement row")?,
            })
        })
        .collect::<Result<Vec<_>, CloseError>>()?;
    Ok(StatementTable {
        title: title.to_owned(),
        rows,
    })
}

fn is_income_sru(sru: Option<i32>) -> bool {
    sru.is_some_and(|code| (3000..=8999).contains(&code))
}
