use crate::{
    Account, Amount, BalanceKey, LedgerSet, SourcePosting, SourceVoucher, SourceYear, balances,
    import_year,
};
use sim_ledger_test_support::{ModelMount, SqliteYearFileFactory};
use std::sync::Arc;

fn mount() -> Arc<dyn sim_storage_port::HostDirPort> {
    Arc::new(ModelMount::new("ledger-model"))
}
fn source(year: i32, source: i64) -> SourceYear {
    SourceYear {
        year,
        accounts: vec![
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
        ],
        vouchers: vec![SourceVoucher {
            source_id: source,
            date: format!("{year}-01-02"),
            text: Some("Sale".into()),
        }],
        postings: vec![
            SourcePosting {
                source_id: source * 10,
                source_voucher_id: source,
                account: 1910,
                amount: Amount(125),
                text: None,
            },
            SourcePosting {
                source_id: source * 10 + 1,
                source_voucher_id: source,
                account: 3010,
                amount: Amount(-125),
                text: None,
            },
        ],
        next_source_voucher_id: 10,
        next_source_posting_id: 20,
    }
}

#[test]
fn model_mount_preserves_content_order_ids_and_balances() {
    let port = mount();
    let factory = Arc::new(SqliteYearFileFactory);
    let mut set = LedgerSet::create(port.clone(), factory.clone(), "Household").unwrap();
    import_year(&mut set, source(2024, 7)).unwrap();
    import_year(&mut set, source(2025, 8)).unwrap();
    let reopened = LedgerSet::open(port, factory).unwrap();
    assert_eq!(reopened.manifest.years, vec![2024, 2025]);
    assert_eq!(
        reopened.year_store(2024).unwrap().vouchers().unwrap()[0].source_id,
        Some(7)
    );
    let rows = balances(&reopened, &[2024, 2025], true).unwrap();
    assert_eq!(
        rows,
        vec![
            crate::BalanceRow {
                key: BalanceKey::Sru { code: 1000 },
                amount: Amount(250),
            },
            crate::BalanceRow {
                key: BalanceKey::Sru { code: 3000 },
                amount: Amount(-250),
            },
        ]
    );
    assert_eq!(
        balances(&reopened, &[2025, 2024], false).unwrap(),
        vec![
            crate::BalanceRow {
                key: BalanceKey::Account {
                    year: 2024,
                    account: 1910,
                },
                amount: Amount(125),
            },
            crate::BalanceRow {
                key: BalanceKey::Account {
                    year: 2024,
                    account: 3010,
                },
                amount: Amount(-125),
            },
            crate::BalanceRow {
                key: BalanceKey::Account {
                    year: 2025,
                    account: 1910,
                },
                amount: Amount(125),
            },
            crate::BalanceRow {
                key: BalanceKey::Account {
                    year: 2025,
                    account: 3010,
                },
                amount: Amount(-125),
            },
        ]
    );
    assert!(balances(&reopened, &[], true).unwrap().is_empty());

    // A report session is assembled by attaching sources after connection. Its
    // successful cross-source query proves cache invalidation, and its main
    // source remains physically read-only even though YearStore has mutations.
    let report = reopened.report_store(&[2024, 2025]).unwrap();
    assert!(matches!(
        report.insert_account(&Account {
            number: 9999,
            name: "Forbidden".into(),
            note: None,
            sru_plus: None,
            sru_minus: None,
        }),
        Err(crate::StoreError::Storage(
            sim_relation_site::SiteError::ReadOnly
        ))
    ));
}

#[test]
fn rejected_import_does_not_advance_manifest() {
    let port = mount();
    let mut set = LedgerSet::create(port, Arc::new(SqliteYearFileFactory), "Household").unwrap();
    let before = set.manifest.clone();
    let mut bad = source(2024, 7);
    bad.postings.pop();
    assert!(import_year(&mut set, bad).is_err());
    assert_eq!(set.manifest, before);
}
