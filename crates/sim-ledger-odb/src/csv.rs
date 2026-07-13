//! CSV front-end for ledger source years.

use std::fmt;
use std::fs::File;
use std::num::ParseIntError;
use std::path::Path;

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
pub fn load_csv(dir: &Path, year: i32, schema: &OdbSchema) -> Result<SourceYear, CsvLoadError> {
    Ok(SourceYear {
        year,
        accounts: read_accounts(dir)?,
        vouchers: read_vouchers(dir)?,
        postings: read_postings(dir)?,
        next_source_voucher_id: restart(schema, "ver")?,
        next_source_posting_id: restart(schema, "trans")?,
    })
}

fn read_accounts(dir: &Path) -> Result<Vec<Account>, CsvLoadError> {
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

fn read_vouchers(dir: &Path) -> Result<Vec<SourceVoucher>, CsvLoadError> {
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

fn read_postings(dir: &Path) -> Result<Vec<SourcePosting>, CsvLoadError> {
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

fn reader(dir: &Path, file: &'static str) -> Result<Reader<File>, CsvLoadError> {
    Ok(ReaderBuilder::new()
        .trim(Trim::All)
        .from_path(dir.join(file))?)
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

#[cfg(test)]
mod tests {
    use std::fs;

    use sim_ledger::{Amount, LedgerSet, Posting, Voucher, YearStore, import_year};

    use super::*;
    use crate::script::parse_script;

    #[test]
    fn csv_export_loads_and_imports_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        write_csvs(dir.path());
        let schema = parse_script(SCRIPT);
        let source = load_csv(dir.path(), 2024, &schema).unwrap();

        assert_eq!(source.next_source_voucher_id, 11_612);
        assert_eq!(source.next_source_posting_id, 25_471);

        let set_dir = dir.path().join("set");
        let mut set = LedgerSet::create(&set_dir, "Household").unwrap();
        import_year(&mut set, source).unwrap();
        let store = YearStore::open(&set.year_path(2024)).unwrap();

        assert_eq!(
            store.vouchers().unwrap(),
            vec![Voucher {
                id: 11_612,
                source_id: Some(11_612),
                date: "2024-01-31".to_owned(),
                text: Some("Receipt".to_owned()),
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
    }

    fn write_csvs(dir: &Path) {
        fs::write(
            dir.join(KONTO),
            "k_nr,k_namn,k_text,k_sru_p,k_sru_m\n1910,Cash,,1000,\n3010,Sales,,,3000\n",
        )
        .unwrap();
        fs::write(
            dir.join(VER),
            "v_nr,v_datum,v_text\n11612,2024-01-31,Receipt\n",
        )
        .unwrap();
        fs::write(
            dir.join(TRANS),
            "t_nr,t_ver,t_konto,t_belopp,t_text\n25471,11612,1910,12.00,Debit\n25472,11612,3010,-12.00,Credit\n",
        )
        .unwrap();
    }

    const SCRIPT: &str = r#"
CREATE CACHED TABLE "konto"("k_nr" INTEGER NOT NULL PRIMARY KEY,"k_namn" VARCHAR(50),"k_text" VARCHAR(200),"k_sru_p" INTEGER,"k_sru_m" INTEGER)
CREATE CACHED TABLE "ver"("v_nr" INTEGER NOT NULL PRIMARY KEY,"v_datum" DATE NOT NULL,"v_text" VARCHAR(200))
CREATE CACHED TABLE "trans"("t_nr" INTEGER NOT NULL PRIMARY KEY,"t_ver" INTEGER NOT NULL,"t_konto" INTEGER NOT NULL,"t_belopp" NUMERIC(50,2) NOT NULL,"t_text" VARCHAR(200))
ALTER TABLE "ver" ALTER COLUMN "v_nr" RESTART WITH 11612
ALTER TABLE "trans" ALTER COLUMN "t_nr" RESTART WITH 25471
"#;
}
