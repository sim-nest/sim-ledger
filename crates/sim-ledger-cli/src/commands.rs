use std::fs;
use std::io::Write;
use std::num::TryFromIntError;
use std::path::Path;

use sim_ledger::{BalanceKey, BalanceRow, LedgerSet, SourceYear, balances, import_year};
use sim_ledger_odb::{load_csv, parse_script, read_odb_for_year};

use crate::args::{Command, ImportSource, ReportGroup, YearSelection};
use crate::error::CliError;

pub(crate) fn execute(command: Command, out: &mut dyn Write) -> Result<(), CliError> {
    match command {
        Command::New { set_dir, label } => create_set(&set_dir, &label, out),
        Command::Import {
            set_dir,
            source,
            year,
        } => import_source(&set_dir, source, year, out),
        Command::Years { set_dir } => list_years(&set_dir, out),
        Command::Report {
            set_dir,
            years,
            group,
        } => report(&set_dir, years, group, out),
    }
}

fn create_set(set_dir: &Path, label: &str, out: &mut dyn Write) -> Result<(), CliError> {
    let set = LedgerSet::create(set_dir, label)?;
    writeln!(
        out,
        "created {} (next voucher id {}, next posting id {})",
        set_dir.display(),
        set.manifest.next_voucher_id,
        set.manifest.next_posting_id
    )?;
    Ok(())
}

fn import_source(
    set_dir: &Path,
    source: ImportSource,
    year: i32,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let mut set = LedgerSet::open(set_dir)?;
    let source = read_source(source, year)?;
    let summary = ImportSummary::from_source(&set, &source)?;
    import_year(&mut set, source)?;
    writeln!(
        out,
        "imported {}: {} accounts, {} vouchers, {} postings",
        summary.year, summary.account_count, summary.voucher_count, summary.posting_count
    )?;
    writeln!(
        out,
        "  canonical voucher ids {}..{}, posting ids {}..{}",
        summary.voucher_start, summary.voucher_end, summary.posting_start, summary.posting_end
    )?;
    Ok(())
}

fn read_source(source: ImportSource, year: i32) -> Result<SourceYear, CliError> {
    match source {
        ImportSource::Odb(path) => Ok(read_odb_for_year(&path, year)?),
        ImportSource::Csv(dir) => {
            let script = read_csv_script(&dir)?;
            let schema = parse_script(&script);
            Ok(load_csv(&dir, year, &schema)?)
        }
    }
}

fn read_csv_script(dir: &Path) -> Result<String, CliError> {
    for candidate in ["database/script", "script"] {
        let path = dir.join(candidate);
        if path.is_file() {
            return Ok(fs::read_to_string(path)?);
        }
    }
    Err(CliError::CsvScriptMissing {
        dir: dir.to_path_buf(),
    })
}

fn list_years(set_dir: &Path, out: &mut dyn Write) -> Result<(), CliError> {
    let set = LedgerSet::open(set_dir)?;
    for year in set.manifest.years {
        writeln!(out, "{year}")?;
    }
    Ok(())
}

fn report(
    set_dir: &Path,
    years: YearSelection,
    group: ReportGroup,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let set = LedgerSet::open(set_dir)?;
    let years = match years {
        YearSelection::All => set.manifest.years.clone(),
        YearSelection::One(year) => vec![year],
    };
    let rows = balances(&set, &years, matches!(group, ReportGroup::Sru))
        .map_err(|source| CliError::Report(source.to_string()))?;
    match group {
        ReportGroup::Account => write_account_report(&rows, out),
        ReportGroup::Sru => write_sru_report(&rows, out),
    }
}

fn write_account_report(rows: &[BalanceRow], out: &mut dyn Write) -> Result<(), CliError> {
    writeln!(out, "YEAR ACCOUNT BALANCE")?;
    for row in rows {
        if let BalanceKey::Account { year, account } = row.key {
            writeln!(out, "{year} {account} {}", row.amount)?;
        }
    }
    Ok(())
}

fn write_sru_report(rows: &[BalanceRow], out: &mut dyn Write) -> Result<(), CliError> {
    writeln!(out, "SRU BALANCE")?;
    for row in rows {
        if let BalanceKey::Sru { code } = row.key {
            writeln!(out, "{code} {}", row.amount)?;
        }
    }
    Ok(())
}

struct ImportSummary {
    year: i32,
    account_count: usize,
    voucher_count: usize,
    posting_count: usize,
    voucher_start: i64,
    voucher_end: i64,
    posting_start: i64,
    posting_end: i64,
}

impl ImportSummary {
    fn from_source(set: &LedgerSet, source: &SourceYear) -> Result<ImportSummary, CliError> {
        let mut preview = set.clone();
        preview.manifest.next_voucher_id = preview
            .manifest
            .next_voucher_id
            .max(source.next_source_voucher_id);
        preview.manifest.next_posting_id = preview
            .manifest
            .next_posting_id
            .max(source.next_source_posting_id);
        let voucher_ids =
            preview.alloc_voucher_ids(count_as_i64("voucher", source.vouchers.len())?)?;
        let posting_ids =
            preview.alloc_posting_ids(count_as_i64("posting", source.postings.len())?)?;
        Ok(ImportSummary {
            year: source.year,
            account_count: source.accounts.len(),
            voucher_count: source.vouchers.len(),
            posting_count: source.postings.len(),
            voucher_start: voucher_ids.start,
            voucher_end: voucher_ids.end,
            posting_start: posting_ids.start,
            posting_end: posting_ids.end,
        })
    }
}

fn count_as_i64(row_kind: &'static str, count: usize) -> Result<i64, CliError> {
    i64::try_from(count).map_err(|_: TryFromIntError| CliError::CountOverflow { row_kind, count })
}
