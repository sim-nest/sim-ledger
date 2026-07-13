use std::fs;
use std::path::Path;

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
