use std::collections::BTreeMap;
use std::io::Write;
use std::num::TryFromIntError;

use sim_ledger::{
    Amount, BalanceKey, BalanceRow, LedgerSet, Posting, SourceYear, balances, import_year,
};
use sim_ledger_odb::{load_csv, parse_script, read_odb_for_year};
use sim_lib_ledger_books::{JournalDraft, validate_draft};
use sim_lib_ledger_close::{
    FinancialStatements, StatementTable, TrialBalanceRow, close_year, compare_by_sru,
    financial_statements,
};

use crate::CommandContext;
use crate::args::{Command, DraftPosting, ImportSource, ReportGroup, YearSelection};
use crate::error::CliError;

pub(crate) fn execute(
    context: &CommandContext,
    command: Command,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    match command {
        Command::New { set_dir, label } => create_set(context, &set_dir, &label, out),
        Command::Import {
            set_dir,
            source,
            year,
        } => import_source(context, &set_dir, source, year, out),
        Command::Years { set_dir } => list_years(context, &set_dir, out),
        Command::Report {
            set_dir,
            years,
            group,
        } => report(context, &set_dir, years, group, out),
        Command::Close { set_dir, year } => close(context, &set_dir, year, out),
        Command::Statements { set_dir, year } => statements(context, &set_dir, year, out),
        Command::SruCompare { set_dir, years } => sru_compare(context, &set_dir, &years, out),
        Command::DraftCheck {
            date,
            text,
            postings,
        } => draft_check(date, text, postings, out),
    }
}

fn mount(
    context: &CommandContext,
    name: &str,
) -> Result<std::sync::Arc<dyn sim_storage_port::HostDirPort>, CliError> {
    context
        .ledger_sets
        .get(name)
        .cloned()
        .ok_or_else(|| CliError::Report(format!("ledger mount {name} is not supplied")))
}
fn create_set(
    context: &CommandContext,
    set_dir: &str,
    label: &str,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let set = LedgerSet::create(mount(context, set_dir)?, context.year_files.clone(), label)?;
    writeln!(
        out,
        "created {} (next voucher id {}, next posting id {})",
        set_dir, set.manifest.next_voucher_id, set.manifest.next_posting_id
    )?;
    Ok(())
}

fn import_source(
    context: &CommandContext,
    set_dir: &str,
    source: ImportSource,
    year: i32,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let mut set = LedgerSet::open(mount(context, set_dir)?, context.year_files.clone())?;
    let source = read_source(context, source, year)?;
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

fn read_source(
    context: &CommandContext,
    source: ImportSource,
    year: i32,
) -> Result<SourceYear, CliError> {
    match source {
        ImportSource::Odb(path) => {
            let mount = import_mount(context, &path)?;
            let bytes = mount
                .read(&["content.odb".into()])
                .map_err(|e| CliError::Report(e.to_string()))?;
            Ok(read_odb_for_year(&bytes, year)?)
        }
        ImportSource::Csv(dir) => {
            let mount = import_mount(context, &dir)?;
            let script = read_csv_script(mount.as_ref())?;
            let schema =
                parse_script(&script).map_err(|error| CliError::Report(error.to_string()))?;
            let mut files = BTreeMap::new();
            for name in ["konto.csv", "ver.csv", "trans.csv"] {
                files.insert(
                    name.into(),
                    mount
                        .read(&[name.into()])
                        .map_err(|e| CliError::Report(e.to_string()))?,
                );
            }
            Ok(load_csv(&files, year, &schema)?)
        }
    }
}

fn import_mount(
    context: &CommandContext,
    path: &str,
) -> Result<std::sync::Arc<dyn sim_storage_port::HostDirPort>, CliError> {
    let key = path;
    context
        .imports
        .get(key)
        .cloned()
        .ok_or_else(|| CliError::Report(format!("import mount {key} is not supplied")))
}
fn read_csv_script(dir: &dyn sim_storage_port::HostDirPort) -> Result<String, CliError> {
    for candidate in ["database/script", "script"] {
        let parts = candidate.split('/').map(str::to_owned).collect::<Vec<_>>();
        if let Ok(bytes) = dir.read(&parts) {
            return String::from_utf8(bytes).map_err(|e| CliError::Report(e.to_string()));
        }
    }
    Err(CliError::CsvScriptMissing {
        mount: dir.label().to_owned(),
    })
}

fn list_years(
    context: &CommandContext,
    set_dir: &str,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let set = LedgerSet::open(mount(context, set_dir)?, context.year_files.clone())?;
    for year in set.manifest.years {
        writeln!(out, "{year}")?;
    }
    Ok(())
}

fn report(
    context: &CommandContext,
    set_dir: &str,
    years: YearSelection,
    group: ReportGroup,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let set = LedgerSet::open(mount(context, set_dir)?, context.year_files.clone())?;
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

fn close(
    context: &CommandContext,
    set_dir: &str,
    year: i32,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let mut set = LedgerSet::open(mount(context, set_dir)?, context.year_files.clone())?;
    let statements = close_year(&mut set, year)?;
    writeln!(out, "closed {year}")?;
    write_financial_statements(&statements, out)
}

fn statements(
    context: &CommandContext,
    set_dir: &str,
    year: i32,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let set = LedgerSet::open(mount(context, set_dir)?, context.year_files.clone())?;
    let statements = financial_statements(&set, year)?;
    write_financial_statements(&statements, out)
}

fn sru_compare(
    context: &CommandContext,
    set_dir: &str,
    years: &[i32],
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let set = LedgerSet::open(mount(context, set_dir)?, context.year_files.clone())?;
    writeln!(
        out,
        "SRU {}",
        years
            .iter()
            .map(i32::to_string)
            .collect::<Vec<_>>()
            .join(" ")
    )?;
    for row in compare_by_sru(&set, years)? {
        write!(out, "{}", row.sru)?;
        for amount in row.years {
            write!(out, " {}", Amount(amount.amount_minor))?;
        }
        writeln!(out)?;
    }
    Ok(())
}

fn draft_check(
    date: time::Date,
    text: String,
    postings: Vec<DraftPosting>,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let posting_count = postings.len();
    let draft = JournalDraft {
        date,
        text,
        postings: draft_postings(postings)?,
        evidence: Vec::new(),
    };
    validate_draft(&draft)?;
    writeln!(
        out,
        "journal draft {} is balanced: {posting_count} postings",
        draft.date
    )?;
    Ok(())
}

fn draft_postings(postings: Vec<DraftPosting>) -> Result<Vec<Posting>, CliError> {
    postings
        .into_iter()
        .enumerate()
        .map(|(index, posting)| {
            let count = index + 1;
            let id =
                i64::try_from(count).map_err(|_: TryFromIntError| CliError::CountOverflow {
                    row_kind: "posting",
                    count,
                })?;
            Ok(Posting {
                id,
                source_id: None,
                voucher_id: 1,
                account: posting.account,
                amount: posting.amount,
                text: None,
            })
        })
        .collect()
}

fn write_financial_statements(
    statements: &FinancialStatements,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    writeln!(out, "YEAR {}", statements.year)?;
    write_trial_balance(&statements.trial_balance, out)?;
    write_statement_table(&statements.income_statement, out)?;
    write_statement_table(&statements.balance_sheet, out)?;
    writeln!(out, "NOTES")?;
    for note in &statements.notes {
        writeln!(out, "{} {}", note.id, note.text)?;
    }
    Ok(())
}

fn write_trial_balance(rows: &[TrialBalanceRow], out: &mut dyn Write) -> Result<(), CliError> {
    writeln!(out, "TRIAL BALANCE")?;
    writeln!(out, "ACCOUNT OPENING DEBIT CREDIT CLOSING SRU")?;
    for row in rows {
        let sru = row
            .closing_sru()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "-".to_owned());
        writeln!(
            out,
            "{} {} {} {} {} {}",
            row.account,
            Amount(row.opening_minor),
            Amount(row.debit_minor),
            Amount(row.credit_minor),
            Amount(row.closing_minor),
            sru
        )?;
    }
    Ok(())
}

fn write_statement_table(table: &StatementTable, out: &mut dyn Write) -> Result<(), CliError> {
    writeln!(out, "{}", table.title.to_ascii_uppercase())?;
    for row in &table.rows {
        writeln!(out, "{} {}", row.label, Amount(row.amount_minor))?;
    }
    writeln!(out, "TOTAL {}", Amount(table.total_minor()?))?;
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
