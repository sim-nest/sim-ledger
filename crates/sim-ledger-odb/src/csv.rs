//! CSV front-end for ledger source years.

use std::collections::BTreeMap;
use std::fmt;
use std::io::Cursor;
use std::num::ParseIntError;

use ::csv::{Reader, ReaderBuilder, StringRecord, Trim};
use sim_ledger::{Account, Amount, SourcePosting, SourceVoucher, SourceYear};

use crate::script::OdbSchema;

const KONTO: &str = "konto.csv";
const VER: &str = "ver.csv";
const TRANS: &str = "trans.csv";

/// Failure while loading ledger CSV exports.
#[derive(Debug)]
pub enum CsvLoadError {
    /// CSV parser or reader failure.
    Csv {
        /// Original CSV error.
        source: Box<::csv::Error>,
    },
    /// A required high-water mark is missing from the schema.
    MissingRestart {
        /// Source table name.
        table: &'static str,
    },
    /// A required CSV field is missing.
    MissingField {
        /// CSV file name.
        file: &'static str,
        /// CSV field name.
        field: &'static str,
    },
    /// An integer field could not be parsed.
    InvalidInteger {
        /// CSV file name.
        file: &'static str,
        /// CSV field name.
        field: &'static str,
        /// Original field value.
        value: String,
        /// Original parse error.
        source: ParseIntError,
    },
    /// An amount field could not be parsed.
    InvalidAmount {
        /// CSV file name.
        file: &'static str,
        /// CSV field name.
        field: &'static str,
        /// Original field value.
        value: String,
        /// Parse failure message.
        message: String,
    },
}

impl fmt::Display for CsvLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CsvLoadError::Csv { source } => write!(f, "CSV load failure: {source}"),
            CsvLoadError::MissingRestart { table } => {
                write!(f, "missing restart high-water mark for table {table}")
            }
            CsvLoadError::MissingField { file, field } => {
                write!(f, "missing field {field} in {file}")
            }
            CsvLoadError::InvalidInteger {
                file,
                field,
                value,
                source,
            } => write!(
                f,
                "invalid integer in {file}:{field} value {value:?}: {source}"
            ),
            CsvLoadError::InvalidAmount {
                file,
                field,
                value,
                message,
            } => write!(
                f,
                "invalid amount in {file}:{field} value {value:?}: {message}"
            ),
        }
    }
}

impl std::error::Error for CsvLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CsvLoadError::Csv { source } => Some(source.as_ref()),
            CsvLoadError::InvalidInteger { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<::csv::Error> for CsvLoadError {
    fn from(source: ::csv::Error) -> CsvLoadError {
        CsvLoadError::Csv {
            source: Box::new(source),
        }
    }
}

/// Load exported `konto.csv`, `ver.csv`, and `trans.csv` into a source year.
pub fn load_csv(
    dir: &BTreeMap<String, Vec<u8>>,
    year: i32,
    schema: &OdbSchema,
) -> Result<SourceYear, CsvLoadError> {
    Ok(SourceYear {
        year,
        accounts: read_accounts(dir)?,
        vouchers: read_vouchers(dir)?,
        postings: read_postings(dir)?,
        next_source_voucher_id: restart(schema, "ver")?,
        next_source_posting_id: restart(schema, "trans")?,
    })
}

fn read_accounts(dir: &BTreeMap<String, Vec<u8>>) -> Result<Vec<Account>, CsvLoadError> {
    let mut reader = reader(dir, KONTO)?;
    let headers = reader.headers()?.clone();
    let mut accounts = Vec::new();
    for record in reader.records() {
        let record = record?;
        accounts.push(Account {
            number: parse_i64(KONTO, "k_nr", field(&headers, &record, KONTO, "k_nr")?)?,
            name: field(&headers, &record, KONTO, "k_namn")?.to_owned(),
            note: optional_text(field(&headers, &record, KONTO, "k_text")?),
            sru_plus: optional_i32(
                KONTO,
                "k_sru_p",
                field(&headers, &record, KONTO, "k_sru_p")?,
            )?,
            sru_minus: optional_i32(
                KONTO,
                "k_sru_m",
                field(&headers, &record, KONTO, "k_sru_m")?,
            )?,
        });
    }
    Ok(accounts)
}

fn read_vouchers(dir: &BTreeMap<String, Vec<u8>>) -> Result<Vec<SourceVoucher>, CsvLoadError> {
    let mut reader = reader(dir, VER)?;
    let headers = reader.headers()?.clone();
    let mut vouchers = Vec::new();
    for record in reader.records() {
        let record = record?;
        vouchers.push(SourceVoucher {
            source_id: parse_i64(VER, "v_nr", field(&headers, &record, VER, "v_nr")?)?,
            date: field(&headers, &record, VER, "v_datum")?.to_owned(),
            text: optional_text(field(&headers, &record, VER, "v_text")?),
        });
    }
    Ok(vouchers)
}

fn read_postings(dir: &BTreeMap<String, Vec<u8>>) -> Result<Vec<SourcePosting>, CsvLoadError> {
    let mut reader = reader(dir, TRANS)?;
    let headers = reader.headers()?.clone();
    let mut postings = Vec::new();
    for record in reader.records() {
        let record = record?;
        let amount_text = field(&headers, &record, TRANS, "t_belopp")?;
        postings.push(SourcePosting {
            source_id: parse_i64(TRANS, "t_nr", field(&headers, &record, TRANS, "t_nr")?)?,
            source_voucher_id: parse_i64(
                TRANS,
                "t_ver",
                field(&headers, &record, TRANS, "t_ver")?,
            )?,
            account: parse_i64(
                TRANS,
                "t_konto",
                field(&headers, &record, TRANS, "t_konto")?,
            )?,
            amount: Amount::parse(amount_text).map_err(|message| CsvLoadError::InvalidAmount {
                file: TRANS,
                field: "t_belopp",
                value: amount_text.to_owned(),
                message,
            })?,
            text: optional_text(field(&headers, &record, TRANS, "t_text")?),
        });
    }
    Ok(postings)
}

fn reader(
    dir: &BTreeMap<String, Vec<u8>>,
    file: &'static str,
) -> Result<Reader<Cursor<Vec<u8>>>, CsvLoadError> {
    let bytes = dir.get(file).cloned().ok_or(CsvLoadError::MissingField {
        file,
        field: "file",
    })?;
    Ok(ReaderBuilder::new()
        .trim(Trim::All)
        .from_reader(Cursor::new(bytes)))
}

fn restart(schema: &OdbSchema, table: &'static str) -> Result<i64, CsvLoadError> {
    schema
        .restart
        .get(table)
        .copied()
        .ok_or(CsvLoadError::MissingRestart { table })
}

fn field<'a>(
    headers: &StringRecord,
    record: &'a StringRecord,
    file: &'static str,
    field: &'static str,
) -> Result<&'a str, CsvLoadError> {
    let index = headers
        .iter()
        .position(|header| header == field)
        .ok_or(CsvLoadError::MissingField { file, field })?;
    record
        .get(index)
        .ok_or(CsvLoadError::MissingField { file, field })
}

fn parse_i64(file: &'static str, field: &'static str, value: &str) -> Result<i64, CsvLoadError> {
    value
        .parse()
        .map_err(|source| CsvLoadError::InvalidInteger {
            file,
            field,
            value: value.to_owned(),
            source,
        })
}

fn optional_i32(
    file: &'static str,
    field: &'static str,
    value: &str,
) -> Result<Option<i32>, CsvLoadError> {
    if value.is_empty() {
        Ok(None)
    } else {
        value
            .parse()
            .map(Some)
            .map_err(|source| CsvLoadError::InvalidInteger {
                file,
                field,
                value: value.to_owned(),
                source,
            })
    }
}

fn optional_text(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}
