use super::*;
use sim_ledger_test_support::{ModelMount, SqliteYearFileFactory};
use sim_storage_port::NeverCancel;

const LEGACY: &[u8] = include_bytes!("../../fixtures/legacy-ledger-year-v1.sqlite");

fn mount() -> Arc<dyn HostDirPort> {
    Arc::new(ModelMount::new("ledger-year-oracle"))
}

#[test]
fn open_and_closed_legacy_year_files_are_complete_oracles() {
    let mount = mount();
    mount
        .replace(&["year-2022.sqlite".into()], LEGACY, &NeverCancel)
        .unwrap();
    let before = mount.read(&["year-2022.sqlite".into()]).unwrap();
    let store = YearStore::open(&SqliteYearFileFactory, mount.clone(), 2022).unwrap();

    assert_eq!(store.accounts().unwrap().len(), 2);
    assert_eq!(
        store
            .vouchers()
            .unwrap()
            .iter()
            .map(|v| v.id)
            .collect::<Vec<_>>(),
        vec![100, 101]
    );
    assert_eq!(store.postings().unwrap().len(), 3);
    assert_eq!(store.id_state_value("voucher").unwrap(), Some(102));
    assert_eq!(
        store.meta_value("source").unwrap().as_deref(),
        Some("legacy")
    );
    assert_eq!(
        store.voucher_balance_violations().unwrap(),
        vec![VoucherBalanceViolation {
            voucher_id: 101,
            posting_count: 1,
            minor_sum: 50
        }]
    );

    let rejected = store.insert_voucher(&Voucher {
        id: 102,
        source_id: None,
        date: "2022-06-01".into(),
        text: None,
    });
    assert!(matches!(rejected, Err(StoreError::Closed)));
    assert_eq!(mount.read(&["year-2022.sqlite".into()]).unwrap(), before);
}

#[test]
fn create_crud_reopen_and_exclusive_lifecycle_are_exact() {
    let mount = mount();
    let factory = SqliteYearFileFactory;
    let store = YearStore::create(&factory, mount.clone(), 2026).unwrap();
    store
        .insert_account(&Account {
            number: 1910,
            name: "Cash".into(),
            note: None,
            sru_plus: Some(1000),
            sru_minus: Some(1000),
        })
        .unwrap();
    store
        .insert_voucher(&Voucher {
            id: 7,
            source_id: Some(70),
            date: "2026-01-02".into(),
            text: Some("Receipt".into()),
        })
        .unwrap();
    store
        .insert_posting(&Posting {
            id: 8,
            source_id: Some(80),
            voucher_id: 7,
            account: 1910,
            amount: Amount(125),
            text: None,
        })
        .unwrap();
    store.set_id_state("voucher", 8).unwrap();
    store.set_meta("source", "oracle").unwrap();
    assert!(matches!(
        YearStore::create(&factory, mount.clone(), 2026),
        Err(StoreError::AlreadyExists)
    ));
    drop(store);

    let reopened = YearStore::open(&factory, mount, 2026).unwrap();
    assert_eq!(reopened.vouchers().unwrap()[0].source_id, Some(70));
    assert_eq!(reopened.postings().unwrap()[0].amount, Amount(125));
    assert_eq!(reopened.id_state_value("voucher").unwrap(), Some(8));
    assert_eq!(
        reopened.meta_value("source").unwrap().as_deref(),
        Some("oracle")
    );
}

#[test]
fn logical_legacy_and_normalized_schema_ids_cross_check() {
    let manifest = legacy_adoption_manifest().unwrap();
    assert_ne!(manifest.logical_schema, manifest.physical_schema);
    assert_eq!(manifest, legacy_adoption_manifest().unwrap());
}
