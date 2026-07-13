use std::fs::File;
use std::io::Write;
use std::path::Path;

use sim_ledger::{Account, Amount, SourcePosting, SourceVoucher};
use tempfile::tempdir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::hsqldb::{Cell, write_cell};
use crate::{ColType, read_odb};

const SCRIPT: &str = "database/script";
const DATA: &str = "database/data";
const PROPERTIES: &str = "database/properties";

#[test]
fn reads_synthetic_odb_end_to_end() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("ledger-2024.odb");
    let (data, roots) = synthetic_data();
    let script = script_text(roots);
    write_odb(&path, &script, &data, "hsqldb.cache_file_scale=1\n");

    let year = read_odb(&path).unwrap();

    assert_eq!(year.year, 2024);
    assert_eq!(year.next_source_voucher_id, 11_612);
    assert_eq!(year.next_source_posting_id, 25_471);
    assert_eq!(
        year.accounts,
        vec![
            Account {
                number: 1910,
                name: "Cash".to_owned(),
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
        ]
    );
    assert_eq!(
        year.vouchers,
        vec![SourceVoucher {
            source_id: 11_612,
            date: "2024-01-31".to_owned(),
            text: Some("Receipt".to_owned()),
        }]
    );
    assert_eq!(
        year.postings,
        vec![
            SourcePosting {
                source_id: 25_471,
                source_voucher_id: 11_612,
                account: 1910,
                amount: Amount(1_200),
                text: Some("Debit".to_owned()),
            },
            SourcePosting {
                source_id: 25_472,
                source_voucher_id: 11_612,
                account: 3010,
                amount: Amount(-1_200),
                text: Some("Credit".to_owned()),
            },
        ]
    );
}

fn synthetic_data() -> (Vec<u8>, Roots) {
    let mut data = vec![0; 16];
    let account_cash = append_row(
        &mut data,
        0,
        0,
        &[
            (Cell::Int(1910), ColType::Integer),
            (Cell::Str("Cash".to_owned()), ColType::Varchar),
            (Cell::Null, ColType::Varchar),
            (Cell::Int(1000), ColType::Integer),
            (Cell::Null, ColType::Integer),
        ],
    );
    let account_sales = append_row(
        &mut data,
        account_cash,
        0,
        &[
            (Cell::Int(3010), ColType::Integer),
            (Cell::Str("Sales".to_owned()), ColType::Varchar),
            (Cell::Null, ColType::Varchar),
            (Cell::Null, ColType::Integer),
            (Cell::Int(3000), ColType::Integer),
        ],
    );
    let voucher = append_row(
        &mut data,
        0,
        0,
        &[
            (Cell::Int(11_612), ColType::Integer),
            (Cell::Date("2024-01-31".to_owned()), ColType::Date),
            (Cell::Str("Receipt".to_owned()), ColType::Varchar),
        ],
    );
    let posting_debit = append_row(
        &mut data,
        0,
        0,
        &[
            (Cell::Int(25_471), ColType::Integer),
            (Cell::Int(11_612), ColType::Integer),
            (Cell::Int(1910), ColType::Integer),
            (Cell::Num(1_200), ColType::Numeric),
            (Cell::Str("Debit".to_owned()), ColType::Varchar),
        ],
    );
    let posting_credit = append_row(
        &mut data,
        posting_debit,
        0,
        &[
            (Cell::Int(25_472), ColType::Integer),
            (Cell::Int(11_612), ColType::Integer),
            (Cell::Int(3010), ColType::Integer),
            (Cell::Num(-1_200), ColType::Numeric),
            (Cell::Str("Credit".to_owned()), ColType::Varchar),
        ],
    );
    let posting_sparse = append_row(
        &mut data,
        posting_credit,
        0,
        &[
            (Cell::Int(25_473), ColType::Integer),
            (Cell::Int(11_612), ColType::Integer),
            (Cell::Null, ColType::Integer),
            (Cell::Null, ColType::Numeric),
            (Cell::Str("Draft".to_owned()), ColType::Varchar),
        ],
    );
    (
        data,
        Roots {
            konto: account_sales,
            ver: voucher,
            trans: posting_sparse,
        },
    )
}

fn append_row(data: &mut Vec<u8>, left: i32, right: i32, cells: &[(Cell, ColType)]) -> i32 {
    let offset = i32::try_from(data.len()).unwrap();
    let mut body = Vec::new();
    body.extend_from_slice(&0_i32.to_be_bytes());
    body.extend_from_slice(&left.to_be_bytes());
    body.extend_from_slice(&right.to_be_bytes());
    body.extend_from_slice(&0_i32.to_be_bytes());
    for (cell, ty) in cells {
        write_cell(&mut body, cell, *ty);
    }
    let row_size = i32::try_from(body.len() + 4).unwrap();
    data.extend_from_slice(&row_size.to_be_bytes());
    data.extend_from_slice(&body);
    offset
}

fn write_odb(path: &Path, script: &str, data: &[u8], properties: &str) {
    let file = File::create(path).unwrap();
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    zip.start_file(SCRIPT, options).unwrap();
    zip.write_all(script.as_bytes()).unwrap();
    zip.start_file(DATA, options).unwrap();
    zip.write_all(data).unwrap();
    zip.start_file(PROPERTIES, options).unwrap();
    zip.write_all(properties.as_bytes()).unwrap();
    zip.finish().unwrap();
}

#[derive(Clone, Copy)]
struct Roots {
    konto: i32,
    ver: i32,
    trans: i32,
}

fn script_text(roots: Roots) -> String {
    format!(
        r#"
CREATE CACHED TABLE "konto"("k_nr" INTEGER NOT NULL PRIMARY KEY,"k_namn" VARCHAR(50),"k_text" VARCHAR(200),"k_sru_p" INTEGER,"k_sru_m" INTEGER)
CREATE CACHED TABLE "ver"("v_nr" INTEGER NOT NULL PRIMARY KEY,"v_datum" DATE NOT NULL,"v_text" VARCHAR(200))
CREATE CACHED TABLE "trans"("t_nr" INTEGER NOT NULL PRIMARY KEY,"v_nr" INTEGER NOT NULL,"k_nr" INTEGER NOT NULL,"t_belopp" NUMERIC(50,2) NOT NULL,"t_text" VARCHAR(200))
ALTER TABLE "ver" ALTER COLUMN "v_nr" RESTART WITH 11612
ALTER TABLE "trans" ALTER COLUMN "t_nr" RESTART WITH 25471
SET TABLE "konto" INDEX'{} 0'
SET TABLE "ver" INDEX'{} 11612'
SET TABLE "trans" INDEX'{} 25471'
"#,
        roots.konto, roots.ver, roots.trans
    )
}
