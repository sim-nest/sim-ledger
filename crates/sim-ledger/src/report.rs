//! Deterministic cross-year reports over supplied ledger content.
#![allow(missing_docs)]

use crate::{Amount, LedgerSet, StoreError};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BalanceKey {
    Account { year: i32, account: i64 },
    Sru { code: i32 },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BalanceRow {
    pub key: BalanceKey,
    pub amount: Amount,
}

pub fn balances(
    set: &LedgerSet,
    years: &[i32],
    by_sru: bool,
) -> Result<Vec<BalanceRow>, StoreError> {
    if years.is_empty() {
        return Ok(vec![]);
    }
    let mut rows = set.report_store(years)?.report_balances(years, by_sru)?;
    if by_sru {
        let mut sums = BTreeMap::<i64, i128>::new();
        for row in rows {
            *sums.entry(row.key).or_default() += i128::from(row.amount);
        }
        return sums
            .into_iter()
            .filter(|(_, amount)| *amount != 0)
            .map(|(key, amount)| {
                Ok(BalanceRow {
                    key: BalanceKey::Sru {
                        code: i32::try_from(key)
                            .map_err(|_| StoreError::Malformed("SRU code exceeds i32".into()))?,
                    },
                    amount: Amount(i64::try_from(amount).map_err(|_| {
                        StoreError::Malformed("balance overflows minor units".into())
                    })?),
                })
            })
            .collect();
    }
    rows.sort_by_key(|row| (row.year, row.key));
    rows.into_iter()
        .map(|row| {
            Ok(BalanceRow {
                key: BalanceKey::Account {
                    year: row.year,
                    account: row.key,
                },
                amount: Amount(row.amount),
            })
        })
        .collect()
}
