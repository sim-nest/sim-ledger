//! ODB reader for LibreOffice Base ledger files.

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::path::Path;
use std::str;

use sim_ledger::{Account, Amount, SourcePosting, SourceVoucher, SourceYear};

use crate::hsqldb::{Cell, HsqlError, read_table};
use crate::open_zip_member;
use crate::script::{OdbSchema, parse_script};

const SCRIPT: &str = "database/script";
const DATA: &str = "database/data";
const PROPERTIES: &str = "database/properties";

/// Failure while reading an ODB ledger export.
#[derive(Debug)]
pub enum OdbError {
    /// ZIP or filesystem failure.
    Io {
        /// Original IO error.
        source: io::Error,
    },
    /// ODB text member was not UTF-8.
    Utf8 {
        /// Member name.
        member: &'static str,
        /// Original UTF-8 error.
        source: str::Utf8Error,
    },
    /// HSQLDB table decoding failed.
    Hsql {
        /// Table name being decoded.
        table: &'static str,
        /// Original HSQLDB error.
        source: HsqlError,
    },
    /// The expected HSQLDB cache scale is absent.
    UnsupportedCacheScale,
    /// Required table metadata is missing from the script.
    MissingTable {
        /// Table name.
        table: &'static str,
    },
    /// Required table index metadata is missing from the script.
    MissingIndex {
        /// Table name.
        table: &'static str,
    },
    /// Required high-water mark metadata is missing from the script.
    MissingRestart {
        /// Table name.
        table: &'static str,
    },
    /// A decoded row is missing a required column.
    MissingColumn {
        /// Table name.
        table: &'static str,
        /// Column name.
        column: &'static str,
    },
    /// A decoded cell has the wrong type for its target field.
    WrongCellType {
        /// Table name.
        table: &'static str,
        /// Column name.
        column: &'static str,
    },
    /// A source integer cannot fit in the target type.
    IntegerOutOfRange {
        /// Table name.
        table: &'static str,
        /// Column name.
        column: &'static str,
        /// Source value.
        value: i64,
    },
    /// The ODB path does not contain a ledger year.
    MissingYear,
}

impl fmt::Display for OdbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OdbError::Io { source } => write!(f, "ODB IO failure: {source}"),
            OdbError::Utf8 { member, source } => {
                write!(f, "ODB member {member} is not UTF-8: {source}")
            }
            OdbError::Hsql { table, source } => {
                write!(f, "failed to decode HSQLDB table {table}: {source}")
            }
            OdbError::UnsupportedCacheScale => {
                write!(f, "ODB uses an unsupported hsqldb.cache_file_scale")
            }
            OdbError::MissingTable { table } => {
                write!(f, "missing HSQLDB table {table}")
            }
            OdbError::MissingIndex { table } => {
                write!(f, "missing HSQLDB index root for table {table}")
            }
            OdbError::MissingRestart { table } => {
                write!(f, "missing HSQLDB restart marker for table {table}")
            }
            OdbError::MissingColumn { table, column } => {
                write!(f, "missing column {column} in table {table}")
            }
            OdbError::WrongCellType { table, column } => {
                write!(f, "wrong cell type for {table}.{column}")
            }
            OdbError::IntegerOutOfRange {
                table,
                column,
                value,
            } => write!(f, "integer {value} in {table}.{column} is out of range"),
            OdbError::MissingYear => write!(f, "ODB path does not include a ledger year"),
        }
    }
}

impl std::error::Error for OdbError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            OdbError::Io { source } => Some(source),
            OdbError::Utf8 { source, .. } => Some(source),
            OdbError::Hsql { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<io::Error> for OdbError {
    fn from(source: io::Error) -> OdbError {
        OdbError::Io { source }
    }
}

/// Read a LibreOffice Base `.odb` ledger export into a source year.
pub fn read_odb(path: &Path) -> Result<SourceYear, OdbError> {
    let script_bytes = open_zip_member(path, SCRIPT)?;
    let data = open_zip_member(path, DATA)?;
    let properties_bytes = open_zip_member(path, PROPERTIES)?;
    let script = str::from_utf8(&script_bytes).map_err(|source| OdbError::Utf8 {
        member: SCRIPT,
        source,
    })?;
    let properties = str::from_utf8(&properties_bytes).map_err(|source| OdbError::Utf8 {
        member: PROPERTIES,
        source,
    })?;
    ensure_cache_scale_one(properties)?;

    let schema = parse_script(script);
    let year = year_from_path(path)?;
    let accounts = read_accounts(&data, &schema)?;
    let vouchers = read_vouchers(&data, &schema)?;
    let postings = read_postings(&data, &schema)?;
    Ok(SourceYear {
        year,
        accounts,
        vouchers,
        postings,
        next_source_voucher_id: restart(&schema, "ver")?,
        next_source_posting_id: restart(&schema, "trans")?,
    })
}

fn ensure_cache_scale_one(properties: &str) -> Result<(), OdbError> {
    if properties
        .lines()
        .map(str::trim)
        .any(|line| line == "hsqldb.cache_file_scale=1")
    {
        Ok(())
    } else {
        Err(OdbError::UnsupportedCacheScale)
    }
}

fn read_accounts(data: &[u8], schema: &OdbSchema) -> Result<Vec<Account>, OdbError> {
    let rows = table_rows(data, schema, "konto")?;
    let columns = column_index(schema, "konto")?;
    rows.iter()
        .map(|row| {
            Ok(Account {
                number: int_cell(row, &columns, "konto", "k_nr")?,
                name: string_cell(row, &columns, "konto", "k_namn")?,
                note: optional_string_cell(row, &columns, "konto", "k_text")?,
                sru_plus: optional_i32_cell(row, &columns, "konto", "k_sru_p")?,
                sru_minus: optional_i32_cell(row, &columns, "konto", "k_sru_m")?,
            })
        })
        .collect()
}

fn read_vouchers(data: &[u8], schema: &OdbSchema) -> Result<Vec<SourceVoucher>, OdbError> {
    let rows = table_rows(data, schema, "ver")?;
    let columns = column_index(schema, "ver")?;
    rows.iter()
        .map(|row| {
            Ok(SourceVoucher {
                source_id: int_cell(row, &columns, "ver", "v_nr")?,
                date: date_cell(row, &columns, "ver", "v_datum")?,
                text: optional_string_cell(row, &columns, "ver", "v_text")?,
            })
        })
        .collect()
}

fn read_postings(data: &[u8], schema: &OdbSchema) -> Result<Vec<SourcePosting>, OdbError> {
    let rows = table_rows(data, schema, "trans")?;
    let columns = column_index(schema, "trans")?;
    rows.iter()
        .map(|row| {
            Ok(SourcePosting {
                source_id: int_cell(row, &columns, "trans", "t_nr")?,
                source_voucher_id: int_cell(row, &columns, "trans", "t_ver")?,
                account: int_cell(row, &columns, "trans", "t_konto")?,
                amount: Amount(num_cell(row, &columns, "trans", "t_belopp")?),
                text: optional_string_cell(row, &columns, "trans", "t_text")?,
            })
        })
        .collect()
}

fn table_rows(
    data: &[u8],
    schema: &OdbSchema,
    table: &'static str,
) -> Result<Vec<Vec<Cell>>, OdbError> {
    let cols = schema
        .columns
        .get(table)
        .ok_or(OdbError::MissingTable { table })?;
    let roots = schema
        .index_roots
        .get(table)
        .ok_or(OdbError::MissingIndex { table })?;
    let root = *roots.first().ok_or(OdbError::MissingIndex { table })?;
    read_table(data, root, cols, roots.len()).map_err(|source| OdbError::Hsql { table, source })
}

fn column_index<'a>(
    schema: &'a OdbSchema,
    table: &'static str,
) -> Result<HashMap<&'a str, usize>, OdbError> {
    let cols = schema
        .columns
        .get(table)
        .ok_or(OdbError::MissingTable { table })?;
    Ok(cols
        .iter()
        .enumerate()
        .map(|(index, (name, _))| (name.as_str(), index))
        .collect())
}

fn restart(schema: &OdbSchema, table: &'static str) -> Result<i64, OdbError> {
    schema
        .restart
        .get(table)
        .copied()
        .ok_or(OdbError::MissingRestart { table })
}

fn int_cell(
    row: &[Cell],
    columns: &HashMap<&str, usize>,
    table: &'static str,
    column: &'static str,
) -> Result<i64, OdbError> {
    match cell(row, columns, table, column)? {
        Cell::Int(value) => Ok(*value),
        _ => Err(OdbError::WrongCellType { table, column }),
    }
}

fn optional_i32_cell(
    row: &[Cell],
    columns: &HashMap<&str, usize>,
    table: &'static str,
    column: &'static str,
) -> Result<Option<i32>, OdbError> {
    match cell(row, columns, table, column)? {
        Cell::Null => Ok(None),
        Cell::Int(value) => {
            i32::try_from(*value)
                .map(Some)
                .map_err(|_| OdbError::IntegerOutOfRange {
                    table,
                    column,
                    value: *value,
                })
        }
        _ => Err(OdbError::WrongCellType { table, column }),
    }
}

fn string_cell(
    row: &[Cell],
    columns: &HashMap<&str, usize>,
    table: &'static str,
    column: &'static str,
) -> Result<String, OdbError> {
    match cell(row, columns, table, column)? {
        Cell::Str(value) => Ok(value.clone()),
        _ => Err(OdbError::WrongCellType { table, column }),
    }
}

fn optional_string_cell(
    row: &[Cell],
    columns: &HashMap<&str, usize>,
    table: &'static str,
    column: &'static str,
) -> Result<Option<String>, OdbError> {
    match cell(row, columns, table, column)? {
        Cell::Null => Ok(None),
        Cell::Str(value) if value.is_empty() => Ok(None),
        Cell::Str(value) => Ok(Some(value.clone())),
        _ => Err(OdbError::WrongCellType { table, column }),
    }
}

fn date_cell(
    row: &[Cell],
    columns: &HashMap<&str, usize>,
    table: &'static str,
    column: &'static str,
) -> Result<String, OdbError> {
    match cell(row, columns, table, column)? {
        Cell::Date(value) => Ok(value.clone()),
        _ => Err(OdbError::WrongCellType { table, column }),
    }
}

fn num_cell(
    row: &[Cell],
    columns: &HashMap<&str, usize>,
    table: &'static str,
    column: &'static str,
) -> Result<i64, OdbError> {
    match cell(row, columns, table, column)? {
        Cell::Num(value) => Ok(*value),
        _ => Err(OdbError::WrongCellType { table, column }),
    }
}

fn cell<'a>(
    row: &'a [Cell],
    columns: &HashMap<&str, usize>,
    table: &'static str,
    column: &'static str,
) -> Result<&'a Cell, OdbError> {
    let index = *columns
        .get(column)
        .ok_or(OdbError::MissingColumn { table, column })?;
    row.get(index)
        .ok_or(OdbError::MissingColumn { table, column })
}

fn year_from_path(path: &Path) -> Result<i32, OdbError> {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or(OdbError::MissingYear)?;
    let digits_rev: String = stem
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect();
    if digits_rev.is_empty() {
        return Err(OdbError::MissingYear);
    }
    let year = digits_rev.chars().rev().collect::<String>();
    year.parse().map_err(|_| OdbError::MissingYear)
}
