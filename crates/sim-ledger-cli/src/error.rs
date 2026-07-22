use std::fmt;
use std::path::PathBuf;

use sim_ledger::{IdAllocationError, ImportError};
use sim_ledger_odb::{CsvLoadError, OdbError};
use sim_lib_ledger_books::BooksError;
use sim_lib_ledger_close::CloseError;

#[derive(Debug)]
pub(crate) enum CliError {
    Io(std::io::Error),
    Report(String),
    Import(ImportError),
    Close(CloseError),
    Books(BooksError),
    Csv(CsvLoadError),
    Odb(OdbError),
    CsvScriptMissing {
        dir: PathBuf,
    },
    CountOverflow {
        row_kind: &'static str,
        count: usize,
    },
    IdAllocation {
        source: IdAllocationError,
    },
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Io(source) => write!(f, "{source}"),
            CliError::Report(source) => write!(f, "SQLite report failure: {source}"),
            CliError::Import(ImportError::Unbalanced {
                voucher,
                posting_count,
                minor_sum,
            }) => write!(
                f,
                "import rejected: voucher {voucher} has {posting_count} postings and is unbalanced by {minor_sum} minor units"
            ),
            CliError::Import(source) => write!(f, "{source}"),
            CliError::Close(source) => write!(f, "{source}"),
            CliError::Books(source) => write!(f, "{source}"),
            CliError::Csv(source) => write!(f, "{source}"),
            CliError::Odb(source) => write!(f, "{source}"),
            CliError::CsvScriptMissing { dir } => write!(
                f,
                "CSV import directory {} must contain database/script or script",
                dir.display()
            ),
            CliError::CountOverflow { row_kind, count } => {
                write!(f, "{row_kind} count {count} cannot fit in i64")
            }
            CliError::IdAllocation { source } => {
                write!(f, "{source}")
            }
        }
    }
}

impl std::error::Error for CliError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CliError::Io(source) => Some(source),
            CliError::Report(_) => None,
            CliError::Import(source) => Some(source),
            CliError::Close(source) => Some(source),
            CliError::Books(source) => Some(source),
            CliError::Csv(source) => Some(source),
            CliError::Odb(source) => Some(source),
            CliError::IdAllocation { source } => Some(source),
            _ => None,
        }
    }
}

impl From<std::io::Error> for CliError {
    fn from(source: std::io::Error) -> CliError {
        CliError::Io(source)
    }
}

impl From<ImportError> for CliError {
    fn from(source: ImportError) -> CliError {
        CliError::Import(source)
    }
}

impl From<CloseError> for CliError {
    fn from(source: CloseError) -> CliError {
        CliError::Close(source)
    }
}

impl From<BooksError> for CliError {
    fn from(source: BooksError) -> CliError {
        CliError::Books(source)
    }
}

impl From<IdAllocationError> for CliError {
    fn from(source: IdAllocationError) -> CliError {
        CliError::IdAllocation { source }
    }
}

impl From<CsvLoadError> for CliError {
    fn from(source: CsvLoadError) -> CliError {
        CliError::Csv(source)
    }
}

impl From<OdbError> for CliError {
    fn from(source: OdbError) -> CliError {
        CliError::Odb(source)
    }
}
