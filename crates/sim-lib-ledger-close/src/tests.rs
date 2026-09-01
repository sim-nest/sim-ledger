use crate::{
    ClosingState, TrialBalanceRow, close_state, close_year, financial_statements, trial_balance,
};
use sim_ledger::{Account, Amount, LedgerSet, Posting, Voucher};
use sim_ledger_test_support::{ModelMount, SqliteYearFileFactory};
use std::sync::Arc;

fn set() -> LedgerSet {
    let mount: Arc<dyn sim_storage_port::HostDirPort> = Arc::new(ModelMount::new("close-model"));
    let mut set = LedgerSet::create(mount, Arc::new(SqliteYearFileFactory), "Close").unwrap();
    let store = set.create_year_store(2026).unwrap();
    for row in [
        Account {
            number: 1910,
            name: "Cash".into(),
            note: None,
            sru_plus: Some(1000),
            sru_minus: Some(1000),
        },
        Account {
            number: 3010,
            name: "Sales".into(),
            note: None,
            sru_plus: Some(3000),
            sru_minus: Some(3000),
        },
    ] {
        store.insert_account(&row).unwrap()
    }
    store
        .insert_voucher(&Voucher {
            id: 1,
            source_id: None,
            date: "2026-01-01".into(),
            text: None,
        })
        .unwrap();
    for (id, account, minor) in [(1, 1910, 500), (2, 3010, -500)] {
        store
            .insert_posting(&Posting {
                id,
                source_id: None,
                voucher_id: 1,
                account,
                amount: Amount(minor),
                text: None,
            })
            .unwrap()
    }
    set.manifest.years.push(2026);
    set.save().unwrap();
    set
}
#[test]
fn model_content_closes_without_host_time_or_paths() {
    let mut set = set();
    assert_eq!(
        financial_statements(&set, 2026)
            .unwrap()
            .trial_balance_total_minor()
            .unwrap(),
        0
    );
    close_year(&mut set, 2026).unwrap();
    assert_eq!(close_state(&set, 2026).unwrap(), ClosingState::Closed);
    assert!(
        set.year_store(2026)
            .unwrap()
            .insert_posting(&Posting {
                id: 3,
                source_id: None,
                voucher_id: 1,
                account: 1910,
                amount: Amount(0),
                text: None
            })
            .is_err()
    );
}

#[test]
fn trial_balance_freezes_posted_empty_and_closed_year_oracles() {
    let mut set = set();
    assert_eq!(
        trial_balance(&set, 2026).unwrap(),
        vec![
            TrialBalanceRow {
                year: 2026,
                account: 1910,
                sru_plus: Some(1000),
                sru_minus: Some(1000),
                opening_minor: 0,
                debit_minor: 500,
                credit_minor: 0,
                closing_minor: 500,
            },
            TrialBalanceRow {
                year: 2026,
                account: 3010,
                sru_plus: Some(3000),
                sru_minus: Some(3000),
                opening_minor: 0,
                debit_minor: 0,
                credit_minor: 500,
                closing_minor: -500,
            },
        ]
    );
    let empty = set.create_year_store(2025).unwrap();
    empty
        .insert_account(&Account {
            number: 1910,
            name: "Cash".into(),
            note: None,
            sru_plus: Some(1000),
            sru_minus: Some(1000),
        })
        .unwrap();
    assert_eq!(
        trial_balance(&set, 2025).unwrap(),
        vec![TrialBalanceRow {
            year: 2025,
            account: 1910,
            sru_plus: Some(1000),
            sru_minus: Some(1000),
            opening_minor: 0,
            debit_minor: 0,
            credit_minor: 0,
            closing_minor: 0,
        }]
    );
    close_year(&mut set, 2026).unwrap();
    assert_eq!(trial_balance(&set, 2026).unwrap()[1].closing_minor, -500);
}
