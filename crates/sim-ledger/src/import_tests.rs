use std::fs;

use crate::{
    Account, Amount, IdAllocationError, ImportError, LedgerSet, Posting, SourcePosting,
    SourceVoucher, SourceYear, Voucher, YearStore, import_year,
};

#[test]
fn imports_source_year_with_carried_ids() {
    let dir = tempfile::tempdir().unwrap();
    let mut set = LedgerSet::create(dir.path(), "Household").unwrap();

    import_year(&mut set, balanced_source_year(2024, 11_612, 25_471, 1_200)).unwrap();

    assert_eq!(set.manifest.next_voucher_id, 11_613);
    assert_eq!(set.manifest.next_posting_id, 25_473);
    assert_eq!(set.manifest.years, vec![2024]);

    let store = YearStore::open(&set.year_path(2024)).unwrap();
    assert_eq!(
        store.vouchers().unwrap(),
        vec![Voucher {
            id: 11_612,
            source_id: Some(11_612),
            date: "2024-01-31".to_owned(),
            text: Some("Source voucher".to_owned()),
        }]
    );
    assert_eq!(
        store.postings().unwrap(),
        vec![
            Posting {
                id: 25_471,
                source_id: Some(25_471),
                voucher_id: 11_612,
                account: 1910,
                amount: Amount(1_200),
                text: Some("Debit".to_owned()),
            },
            Posting {
                id: 25_472,
                source_id: Some(25_472),
                voucher_id: 11_612,
                account: 3010,
                amount: Amount(-1_200),
                text: Some("Credit".to_owned()),
            },
        ]
    );
    assert_eq!(id_state(&store, "voucher"), 11_613);
    assert_eq!(id_state(&store, "posting"), 25_473);

    let reloaded = LedgerSet::open(dir.path()).unwrap();
    assert_eq!(reloaded.manifest, set.manifest);
}

#[test]
fn later_import_keeps_existing_cursor_when_source_cursor_is_lower() {
    let dir = tempfile::tempdir().unwrap();
    let mut set = LedgerSet::create(dir.path(), "Household").unwrap();

    import_year(&mut set, balanced_source_year(2024, 11_612, 25_471, 1_200)).unwrap();
    import_year(&mut set, balanced_source_year(2025, 20, 30, 800)).unwrap();

    assert_eq!(set.manifest.next_voucher_id, 11_614);
    assert_eq!(set.manifest.next_posting_id, 25_475);
    assert_eq!(set.manifest.years, vec![2024, 2025]);

    let second = YearStore::open(&set.year_path(2025)).unwrap();
    assert_eq!(second.vouchers().unwrap()[0].id, 11_613);
    assert_eq!(second.vouchers().unwrap()[0].source_id, Some(20));
    let postings = second.postings().unwrap();
    assert_eq!(postings[0].id, 25_473);
    assert_eq!(postings[0].source_id, Some(30));
    assert_eq!(postings[1].id, 25_474);
    assert_eq!(postings[1].source_id, Some(31));
    assert_eq!(id_state(&second, "voucher"), 11_614);
    assert_eq!(id_state(&second, "posting"), 25_475);
}

#[test]
fn unbalanced_source_year_is_rejected_without_mutating_set() {
    let dir = tempfile::tempdir().unwrap();
    let mut set = LedgerSet::create(dir.path(), "Household").unwrap();
    let mut source = balanced_source_year(2024, 11_612, 25_471, 1_200);
    source.postings[1].amount = Amount(-1_199);

    let err = import_year(&mut set, source).unwrap_err();

    assert!(matches!(
        err,
        ImportError::Unbalanced {
            voucher: 11_612,
            posting_count: 2,
            minor_sum: 1
        }
    ));
    assert_eq!(set.manifest.next_voucher_id, 1);
    assert_eq!(set.manifest.next_posting_id, 1);
    assert!(set.manifest.years.is_empty());
    assert!(!set.year_path(2024).exists());
}

#[test]
fn source_year_with_empty_voucher_is_rejected_without_mutating_set() {
    let dir = tempfile::tempdir().unwrap();
    let mut set = LedgerSet::create(dir.path(), "Household").unwrap();
    let mut source = balanced_source_year(2024, 11_612, 25_471, 1_200);
    source.postings.clear();

    let err = import_year(&mut set, source).unwrap_err();

    assert!(matches!(
        err,
        ImportError::Unbalanced {
            voucher: 11_612,
            posting_count: 0,
            minor_sum: 0
        }
    ));
    assert_eq!(set.manifest.next_voucher_id, 1);
    assert_eq!(set.manifest.next_posting_id, 1);
    assert!(set.manifest.years.is_empty());
    assert!(!set.year_path(2024).exists());
}

#[test]
fn negative_manifest_cursor_is_rejected_without_mutating_set() {
    let dir = tempfile::tempdir().unwrap();
    let mut set = LedgerSet::create(dir.path(), "Household").unwrap();
    set.manifest.next_voucher_id = -4;
    set.save().unwrap();
    let before = set.manifest.clone();
    let before_text = fs::read_to_string(dir.path().join("ledger-set.toml")).unwrap();
    let mut source = balanced_source_year(2024, 1, 1, 1_200);
    source.next_source_voucher_id = -4;

    let err = import_year(&mut set, source).unwrap_err();

    assert!(matches!(
        err,
        ImportError::IdAllocation {
            source: IdAllocationError::NegativeCursor {
                row_kind: "voucher",
                start: -4,
            }
        }
    ));
    assert_eq!(set.manifest, before);
    assert_eq!(
        fs::read_to_string(dir.path().join("ledger-set.toml")).unwrap(),
        before_text
    );
    assert!(!set.year_path(2024).exists());
}

#[test]
fn overflowing_posting_cursor_is_rejected_without_mutating_set() {
    let dir = tempfile::tempdir().unwrap();
    let mut set = LedgerSet::create(dir.path(), "Household").unwrap();
    set.manifest.next_posting_id = i64::MAX;
    set.save().unwrap();
    let before = set.manifest.clone();
    let before_text = fs::read_to_string(dir.path().join("ledger-set.toml")).unwrap();

    let err = import_year(&mut set, balanced_source_year(2024, 1, 1, 1_200)).unwrap_err();

    assert!(matches!(
        err,
        ImportError::IdAllocation {
            source: IdAllocationError::CursorOverflow {
                row_kind: "posting",
                start: i64::MAX,
                count: 2,
            }
        }
    ));
    assert_eq!(set.manifest, before);
    assert_eq!(
        fs::read_to_string(dir.path().join("ledger-set.toml")).unwrap(),
        before_text
    );
    assert!(!set.year_path(2024).exists());
}

fn balanced_source_year(
    year: i32,
    voucher_source_id: i64,
    posting_source_id: i64,
    minor: i64,
) -> SourceYear {
    SourceYear {
        year,
        accounts: vec![account(1910, "Cash"), account(3010, "Sales")],
        vouchers: vec![SourceVoucher {
            source_id: voucher_source_id,
            date: format!("{year}-01-31"),
            text: Some("Source voucher".to_owned()),
        }],
        postings: vec![
            SourcePosting {
                source_id: posting_source_id,
                source_voucher_id: voucher_source_id,
                account: 1910,
                amount: Amount(minor),
                text: Some("Debit".to_owned()),
            },
            SourcePosting {
                source_id: posting_source_id + 1,
                source_voucher_id: voucher_source_id,
                account: 3010,
                amount: Amount(-minor),
                text: Some("Credit".to_owned()),
            },
        ],
        next_source_voucher_id: voucher_source_id,
        next_source_posting_id: posting_source_id,
    }
}

fn account(number: i64, name: &str) -> Account {
    Account {
        number,
        name: name.to_owned(),
        note: None,
        sru_plus: None,
        sru_minus: None,
    }
}

fn id_state(store: &YearStore, kind: &str) -> i64 {
    store
        .conn
        .query_row("SELECT next FROM id_state WHERE kind = ?1", [kind], |row| {
            row.get(0)
        })
        .unwrap()
}
