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
    if by_sru {
        sru_balances(set, years)
    } else {
        account_balances(set, years)
    }
}
fn account_balances(set: &LedgerSet, years: &[i32]) -> Result<Vec<BalanceRow>, StoreError> {
    let mut sums = BTreeMap::<(i32, i64), i128>::new();
    for &year in years {
        for posting in set.year_store(year)?.postings()? {
            *sums.entry((year, posting.account)).or_default() += i128::from(posting.amount.0);
        }
    }
    sums.into_iter()
        .map(|((year, account), sum)| {
            Ok(BalanceRow {
                key: BalanceKey::Account { year, account },
                amount: Amount(exact(sum)?),
            })
        })
        .collect()
}
fn sru_balances(set: &LedgerSet, years: &[i32]) -> Result<Vec<BalanceRow>, StoreError> {
    let mut sums = BTreeMap::<i32, i128>::new();
    for &year in years {
        let store = set.year_store(year)?;
        let accounts = store
            .accounts()?
            .into_iter()
            .map(|a| (a.number, a))
            .collect::<BTreeMap<_, _>>();
        for posting in store.postings()? {
            if let Some(account) = accounts.get(&posting.account) {
                let code = if posting.amount.0 >= 0 {
                    account.sru_plus.or(account.sru_minus)
                } else {
                    account.sru_minus.or(account.sru_plus)
                };
                if let Some(code) = code {
                    *sums.entry(code).or_default() += i128::from(posting.amount.0);
                }
            }
        }
    }
    sums.into_iter()
        .map(|(code, sum)| {
            Ok(BalanceRow {
                key: BalanceKey::Sru { code },
                amount: Amount(exact(sum)?),
            })
        })
        .collect()
}
fn exact(sum: i128) -> Result<i64, StoreError> {
    i64::try_from(sum).map_err(|_| StoreError::Malformed("balance overflows minor units".into()))
}
