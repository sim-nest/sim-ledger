//! Deterministic cookbook builders for ledger recipes.

use std::collections::BTreeMap;

use crate::{Account, Amount, Posting, Voucher, YearData, is_balanced};

/// Account balance row produced by the balanced-year cookbook recipe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CookbookAccountBalance {
    /// Year-local account number.
    pub account: i64,
    /// Account name.
    pub name: String,
    /// Signed balance rendered with two decimal places.
    pub balance: String,
}

/// Report produced by the balanced-year cookbook recipe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BalancedYearDemo {
    /// Ledger year.
    pub year: i32,
    /// Number of modeled accounts.
    pub account_count: usize,
    /// Number of modeled vouchers.
    pub voucher_count: usize,
    /// Number of modeled posting lines.
    pub posting_count: usize,
    /// Whether the voucher posting lines sum to zero.
    pub balanced: bool,
    /// Per-account balances for the modeled year.
    pub balances: Vec<CookbookAccountBalance>,
}

/// Build the modeled balanced-year report used by the cookbook recipe.
#[must_use]
pub fn balanced_year_demo() -> BalancedYearDemo {
    let year = modeled_year();
    let balances = account_balances(&year);

    BalancedYearDemo {
        year: year.year,
        account_count: year.accounts.len(),
        voucher_count: year.vouchers.len(),
        posting_count: year.postings.len(),
        balanced: is_balanced(&year.postings),
        balances,
    }
}

fn modeled_year() -> YearData {
    YearData {
        year: 2024,
        accounts: vec![
            Account {
                number: 1910,
                name: "Bank".to_owned(),
                note: None,
                sru_plus: Some(1000),
                sru_minus: None,
            },
            Account {
                number: 3010,
                name: "Sales".to_owned(),
                note: None,
                sru_plus: None,
                sru_minus: Some(3000),
            },
        ],
        vouchers: vec![Voucher {
            id: 1,
            source_id: Some(1),
            date: "2024-01-31".to_owned(),
            text: Some("Modeled sale".to_owned()),
        }],
        postings: vec![
            Posting {
                id: 1,
                source_id: Some(1),
                voucher_id: 1,
                account: 1910,
                amount: Amount(12_500),
                text: Some("Debit bank".to_owned()),
            },
            Posting {
                id: 2,
                source_id: Some(2),
                voucher_id: 1,
                account: 3010,
                amount: Amount(-12_500),
                text: Some("Credit sales".to_owned()),
            },
        ],
    }
}

fn account_balances(year: &YearData) -> Vec<CookbookAccountBalance> {
    let mut totals = BTreeMap::<i64, i64>::new();
    for posting in &year.postings {
        *totals.entry(posting.account).or_default() += posting.amount.0;
    }

    year.accounts
        .iter()
        .map(|account| CookbookAccountBalance {
            account: account.number,
            name: account.name.clone(),
            balance: Amount(*totals.get(&account.number).unwrap_or(&0)).to_decimal_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balanced_year_demo_checks_double_entry_and_balances() {
        let demo = balanced_year_demo();

        assert_eq!(demo.year, 2024);
        assert_eq!(demo.account_count, 2);
        assert_eq!(demo.voucher_count, 1);
        assert_eq!(demo.posting_count, 2);
        assert!(demo.balanced);
        assert_eq!(
            demo.balances,
            vec![
                CookbookAccountBalance {
                    account: 1910,
                    name: "Bank".to_owned(),
                    balance: "125.00".to_owned(),
                },
                CookbookAccountBalance {
                    account: 3010,
                    name: "Sales".to_owned(),
                    balance: "-125.00".to_owned(),
                },
            ]
        );
    }
}
