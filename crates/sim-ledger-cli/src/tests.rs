use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use sim_ledger_odb::{Cell, ColType, write_cell};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::run;

#[test]
fn csv_loop_imports_years_and_reports() {
    let temp = tempfile::tempdir().unwrap();
    let set_dir = temp.path().join("books");
    let csv_dir = temp.path().join("csv");
    fs::create_dir(&csv_dir).unwrap();
    write_csv_export(&csv_dir, TRANS_BALANCED);

    let (code, out, err) = run_command(["new", path(&set_dir), "--label", "Personal"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.starts_with("created "));
    assert!(out.contains("(next voucher id 1, next posting id 1)\n"));

    let (code, out, err) = run_command([
        "import",
        path(&set_dir),
        "--csv",
        path(&csv_dir),
        "--year",
        "2024",
    ]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        out,
        "imported 2024: 2 accounts, 1 vouchers, 2 postings\n  canonical voucher ids 11612..11613, posting ids 25471..25473\n"
    );

    let (code, out, err) = run_command(["years", path(&set_dir)]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out, "2024\n");

    let (code, out, err) = run_command(["report", path(&set_dir), "--year", "2024", "--by", "sru"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out, "SRU BALANCE\n1000 12.00\n3000 -12.00\n");
}

#[test]
fn odb_import_uses_explicit_year_without_filename_digits() {
    let temp = tempfile::tempdir().unwrap();
    let set_dir = temp.path().join("books");
    let odb_path = temp.path().join("books.odb");
    write_odb_export(&odb_path);

    let (code, out, err) = run_command(["new", path(&set_dir), "--label", "Personal"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.starts_with("created "));

    let (code, out, err) = run_command([
        "import",
        path(&set_dir),
        "--odb",
        path(&odb_path),
        "--year",
        "2024",
    ]);

    assert_eq!(code, 0, "{err}");
    assert_eq!(
        out,
        "imported 2024: 2 accounts, 1 vouchers, 2 postings\n  canonical voucher ids 11612..11613, posting ids 25471..25473\n"
    );

    let (code, out, err) = run_command(["years", path(&set_dir)]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out, "2024\n");
}

#[test]
fn unbalanced_import_names_the_rejected_voucher() {
    let temp = tempfile::tempdir().unwrap();
    let set_dir = temp.path().join("books");
    let csv_dir = temp.path().join("csv");
    fs::create_dir(&csv_dir).unwrap();
    write_csv_export(&csv_dir, TRANS_UNBALANCED);

    let (code, _, err) = run_command(["new", path(&set_dir), "--label", "Personal"]);
    assert_eq!(code, 0, "{err}");

    let (code, _, err) = run_command([
        "import",
        path(&set_dir),
        "--csv",
        path(&csv_dir),
        "--year",
        "2024",
    ]);
    assert_eq!(code, 1);
    assert!(err.contains("voucher 11612"), "{err}");
    assert!(err.contains("unbalanced"), "{err}");
}

#[test]
fn overflowing_import_cursor_reports_error_without_rewriting_manifest() {
    let temp = tempfile::tempdir().unwrap();
    let set_dir = temp.path().join("books");
    let csv_dir = temp.path().join("csv");
    fs::create_dir(&csv_dir).unwrap();
    write_csv_export(&csv_dir, TRANS_BALANCED);

    let (code, _, err) = run_command(["new", path(&set_dir), "--label", "Personal"]);
    assert_eq!(code, 0, "{err}");
    let manifest = format!(
        "label = \"Personal\"\nnext_voucher_id = {}\nnext_posting_id = 1\nyears = []\n",
        i64::MAX
    );
    fs::write(set_dir.join("ledger-set.toml"), &manifest).unwrap();

    let (code, _, err) = run_command([
        "import",
        path(&set_dir),
        "--csv",
        path(&csv_dir),
        "--year",
        "2024",
    ]);

    assert_eq!(code, 1);
    assert!(
        err.contains("voucher id range starting at 9223372036854775807"),
        "{err}"
    );
    assert!(err.contains("overflows i64"), "{err}");
    assert_eq!(
        fs::read_to_string(set_dir.join("ledger-set.toml")).unwrap(),
        manifest
    );
}

fn run_command<const N: usize>(args: [&str; N]) -> (i32, String, String) {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run(args, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

fn path(path: &Path) -> &str {
    path.to_str().unwrap()
}

fn write_csv_export(dir: &Path, trans: &str) {
    fs::write(
        dir.join("script"),
        r#"
CREATE CACHED TABLE "konto"("k_nr" INTEGER NOT NULL PRIMARY KEY,"k_namn" VARCHAR(50),"k_text" VARCHAR(200),"k_sru_p" INTEGER,"k_sru_m" INTEGER)
CREATE CACHED TABLE "ver"("v_nr" INTEGER NOT NULL PRIMARY KEY,"v_datum" DATE NOT NULL,"v_text" VARCHAR(200))
CREATE CACHED TABLE "trans"("t_nr" INTEGER NOT NULL PRIMARY KEY,"t_ver" INTEGER NOT NULL,"t_konto" INTEGER NOT NULL,"t_belopp" NUMERIC(50,2) NOT NULL,"t_text" VARCHAR(200))
ALTER TABLE "ver" ALTER COLUMN "v_nr" RESTART WITH 11612
ALTER TABLE "trans" ALTER COLUMN "t_nr" RESTART WITH 25471
"#,
    )
    .unwrap();
    fs::write(
        dir.join("konto.csv"),
        "k_nr,k_namn,k_text,k_sru_p,k_sru_m\n1910,Cash,,1000,\n3010,Sales,,,3000\n",
    )
    .unwrap();
    fs::write(
        dir.join("ver.csv"),
        "v_nr,v_datum,v_text\n11612,2024-01-31,Receipt\n",
    )
    .unwrap();
    fs::write(dir.join("trans.csv"), trans).unwrap();
}

fn write_odb_export(path: &Path) {
    let (data, roots) = synthetic_odb_data();
    let script = odb_script_text(roots);
    let file = File::create(path).unwrap();
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    zip.start_file("database/script", options).unwrap();
    zip.write_all(script.as_bytes()).unwrap();
    zip.start_file("database/data", options).unwrap();
    zip.write_all(&data).unwrap();
    zip.start_file("database/properties", options).unwrap();
    zip.write_all(b"hsqldb.cache_file_scale=1\n").unwrap();
    zip.finish().unwrap();
}

fn synthetic_odb_data() -> (Vec<u8>, OdbRoots) {
    let mut data = vec![0; 16];
    let account_cash = append_odb_row(
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
    let account_sales = append_odb_row(
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
    let voucher = append_odb_row(
        &mut data,
        0,
        0,
        &[
            (Cell::Int(11_612), ColType::Integer),
            (Cell::Date("2024-01-31".to_owned()), ColType::Date),
            (Cell::Str("Receipt".to_owned()), ColType::Varchar),
        ],
    );
    let posting_debit = append_odb_row(
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
    let posting_credit = append_odb_row(
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
    (
        data,
        OdbRoots {
            konto: account_sales,
            ver: voucher,
            trans: posting_credit,
        },
    )
}

fn append_odb_row(data: &mut Vec<u8>, left: i32, right: i32, cells: &[(Cell, ColType)]) -> i32 {
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

#[derive(Clone, Copy)]
struct OdbRoots {
    konto: i32,
    ver: i32,
    trans: i32,
}

fn odb_script_text(roots: OdbRoots) -> String {
    format!(
        r#"
CREATE CACHED TABLE "konto"("k_nr" INTEGER NOT NULL PRIMARY KEY,"k_namn" VARCHAR(50),"k_text" VARCHAR(200),"k_sru_p" INTEGER,"k_sru_m" INTEGER)
CREATE CACHED TABLE "ver"("v_nr" INTEGER NOT NULL PRIMARY KEY,"v_datum" DATE NOT NULL,"v_text" VARCHAR(200))
CREATE CACHED TABLE "trans"("t_nr" INTEGER NOT NULL PRIMARY KEY,"t_ver" INTEGER NOT NULL,"t_konto" INTEGER NOT NULL,"t_belopp" NUMERIC(50,2) NOT NULL,"t_text" VARCHAR(200))
ALTER TABLE "ver" ALTER COLUMN "v_nr" RESTART WITH 11612
ALTER TABLE "trans" ALTER COLUMN "t_nr" RESTART WITH 25471
SET TABLE "konto" INDEX'{} 0'
SET TABLE "ver" INDEX'{} 11612'
SET TABLE "trans" INDEX'{} 25471'
"#,
        roots.konto, roots.ver, roots.trans
    )
}

const TRANS_BALANCED: &str = "\
t_nr,t_ver,t_konto,t_belopp,t_text
25471,11612,1910,12.00,Debit
25472,11612,3010,-12.00,Credit
";

const TRANS_UNBALANCED: &str = "\
t_nr,t_ver,t_konto,t_belopp,t_text
25471,11612,1910,12.00,Debit
25472,11612,3010,-11.99,Credit
";
