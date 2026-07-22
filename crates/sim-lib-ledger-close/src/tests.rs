use sim_ledger::{Account, Amount, LedgerSet, Posting, Voucher, YearStore};

use super::*;

#[test]
fn trial_balance_totals_zero() {
    let dir = tempfile::tempdir().unwrap();
    let mut set = LedgerSet::create(dir.path(), "Close").unwrap();
    write_year(&mut set, 2026, 1910, 3010, 1_200);

    let rows = trial_balance(&set, 2026).unwrap();
    let total: i64 = rows.iter().map(|row| row.closing_minor).sum();

    assert_eq!(total, 0);
}

#[test]
fn closed_year_rejects_voucher_insert() {
    let dir = tempfile::tempdir().unwrap();
    let mut set = LedgerSet::create(dir.path(), "Close").unwrap();
    write_year(&mut set, 2026, 1910, 3010, 1_200);

    close_year(&mut set, 2026).unwrap();
    let store = YearStore::open(&set.year_path(2026)).unwrap();
    let err = store
        .insert_voucher(&Voucher {
            id: 99,
            source_id: None,
            date: "2026-12-31".to_owned(),
            text: Some("Late voucher".to_owned()),
        })
        .unwrap_err();

    assert!(matches!(err, rusqlite::Error::InvalidQuery));
}

#[test]
fn reopen_records_reason_and_allows_insert() {
    let dir = tempfile::tempdir().unwrap();
    let mut set = LedgerSet::create(dir.path(), "Close").unwrap();
    write_year(&mut set, 2026, 1910, 3010, 1_200);

    close_year(&mut set, 2026).unwrap();
    let journal = reopen_year(&set, 2026, "operator correction").unwrap();

    assert_eq!(journal.last().unwrap().state, ClosingState::Open);
    assert_eq!(journal.last().unwrap().reason, "operator correction");
    let store = YearStore::open(&set.year_path(2026)).unwrap();
    store
        .insert_voucher(&Voucher {
            id: 99,
            source_id: None,
            date: "2026-12-31".to_owned(),
            text: Some("Late voucher".to_owned()),
        })
        .unwrap();
}

#[test]
fn differing_account_numbers_group_by_sru() {
    let dir = tempfile::tempdir().unwrap();
    let mut set = LedgerSet::create(dir.path(), "Close").unwrap();
    write_year(&mut set, 2025, 1910, 3010, 1_200);
    write_year(&mut set, 2026, 1930, 3020, 800);

    let rows = compare_by_sru(&set, &[2025, 2026]).unwrap();

    let asset = rows.iter().find(|row| row.sru == 1000).unwrap();
    assert_eq!(asset.years[0].amount_minor, 1_200);
    assert_eq!(asset.years[1].amount_minor, 800);
    let income = rows.iter().find(|row| row.sru == 3000).unwrap();
    assert_eq!(income.years[0].amount_minor, -1_200);
    assert_eq!(income.years[1].amount_minor, -800);
}

#[test]
fn financial_statement_totals_are_exact() {
    let dir = tempfile::tempdir().unwrap();
    let mut set = LedgerSet::create(dir.path(), "Close").unwrap();
    write_year(&mut set, 2026, 1910, 3010, 1_200);

    let statements = financial_statements(&set, 2026).unwrap();

    assert_eq!(statements.trial_balance_total_minor().unwrap(), 0);
    assert_eq!(statements.balance_sheet.total_minor().unwrap(), 1_200);
    assert_eq!(statements.income_statement.total_minor().unwrap(), -1_200);
}

#[test]
fn close_year_rejects_offsetting_unbalanced_vouchers() {
    let dir = tempfile::tempdir().unwrap();
    let mut set = LedgerSet::create(dir.path(), "Close").unwrap();
    write_offsetting_unbalanced_year(&mut set, 2026);

    let err = close_year(&mut set, 2026).unwrap_err();

    assert!(matches!(
        err,
        CloseError::UnbalancedVoucher {
            voucher: 1,
            posting_count: 2,
            minor_sum: 100,
        }
    ));
    assert_eq!(close_state(&set, 2026).unwrap(), ClosingState::Open);
}

#[test]
fn financial_statements_reject_empty_vouchers() {
    let dir = tempfile::tempdir().unwrap();
    let mut set = LedgerSet::create(dir.path(), "Close").unwrap();
    write_empty_voucher_year(&mut set, 2026);

    let err = financial_statements(&set, 2026).unwrap_err();

    assert!(matches!(
        err,
        CloseError::UnbalancedVoucher {
            voucher: 1,
            posting_count: 0,
            minor_sum: 0,
        }
    ));
}

#[test]
fn sru_comparison_rejects_unbalanced_vouchers() {
    let dir = tempfile::tempdir().unwrap();
    let mut set = LedgerSet::create(dir.path(), "Close").unwrap();
    write_offsetting_unbalanced_year(&mut set, 2026);

    let err = compare_by_sru(&set, &[2026]).unwrap_err();

    assert!(matches!(
        err,
        CloseError::UnbalancedVoucher {
            voucher: 1,
            posting_count: 2,
            minor_sum: 100,
        }
    ));
}

fn write_year(set: &mut LedgerSet, year: i32, debit_account: i64, credit_account: i64, minor: i64) {
    let voucher_id = set.alloc_voucher_ids(1).unwrap().start;
    let posting_ids: Vec<i64> = set.alloc_posting_ids(2).unwrap().collect();
    let store = YearStore::create(&set.year_path(year), year).unwrap();
    store
        .insert_account(&account(debit_account, "Asset", Some(1000), None))
        .unwrap();
    store
        .insert_account(&account(credit_account, "Income", None, Some(3000)))
        .unwrap();
    store
        .insert_voucher(&Voucher {
            id: voucher_id,
            source_id: Some(i64::from(year)),
            date: format!("{year}-12-31"),
            text: Some("Year result".to_owned()),
        })
        .unwrap();
    store
        .insert_posting(&Posting {
            id: posting_ids[0],
            source_id: None,
            voucher_id,
            account: debit_account,
            amount: Amount(minor),
            text: Some("Debit".to_owned()),
        })
        .unwrap();
    store
        .insert_posting(&Posting {
            id: posting_ids[1],
            source_id: None,
            voucher_id,
            account: credit_account,
            amount: Amount(-minor),
            text: Some("Credit".to_owned()),
        })
        .unwrap();
    set.manifest.years.push(year);
    set.save().unwrap();
}

fn write_offsetting_unbalanced_year(set: &mut LedgerSet, year: i32) {
    let voucher_ids: Vec<i64> = set.alloc_voucher_ids(2).unwrap().collect();
    let posting_ids: Vec<i64> = set.alloc_posting_ids(4).unwrap().collect();
    let store = YearStore::create(&set.year_path(year), year).unwrap();
    store
        .insert_account(&account(1910, "Asset", Some(1000), None))
        .unwrap();
    store
        .insert_account(&account(3010, "Income", None, Some(3000)))
        .unwrap();
    insert_voucher(&store, voucher_ids[0], year, "Offset one");
    insert_voucher(&store, voucher_ids[1], year, "Offset two");
    insert_posting(&store, posting_ids[0], voucher_ids[0], 1910, 300);
    insert_posting(&store, posting_ids[1], voucher_ids[0], 3010, -200);
    insert_posting(&store, posting_ids[2], voucher_ids[1], 1910, 200);
    insert_posting(&store, posting_ids[3], voucher_ids[1], 3010, -300);
    set.manifest.years.push(year);
    set.save().unwrap();
}

fn write_empty_voucher_year(set: &mut LedgerSet, year: i32) {
    let voucher_id = set.alloc_voucher_ids(1).unwrap().start;
    let store = YearStore::create(&set.year_path(year), year).unwrap();
    store
        .insert_account(&account(1910, "Asset", Some(1000), None))
        .unwrap();
    insert_voucher(&store, voucher_id, year, "Empty");
    set.manifest.years.push(year);
    set.save().unwrap();
}

fn insert_voucher(store: &YearStore, id: i64, year: i32, text: &str) {
    store
        .insert_voucher(&Voucher {
            id,
            source_id: Some(id),
            date: format!("{year}-12-31"),
            text: Some(text.to_owned()),
        })
        .unwrap();
}

fn insert_posting(store: &YearStore, id: i64, voucher_id: i64, account: i64, minor: i64) {
    store
        .insert_posting(&Posting {
            id,
            source_id: Some(id),
            voucher_id,
            account,
            amount: Amount(minor),
            text: None,
        })
        .unwrap();
}

fn account(number: i64, name: &str, sru_plus: Option<i32>, sru_minus: Option<i32>) -> Account {
    Account {
        number,
        name: name.to_owned(),
        note: None,
        sru_plus,
        sru_minus,
    }
}
