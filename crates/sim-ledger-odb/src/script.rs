//! Admission of SQL-codec HSQLDB schema drafts into the ledger importer domain.

use std::collections::HashMap;
use std::fmt;

use sim_codec_sql::{DdlCodec, LegacyDdl, SchemaDraft, SqlError};

/// Parsed table layout and id high-water marks from `database/script`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OdbSchema {
    /// Column layout by table name.
    pub columns: HashMap<String, Vec<(String, ColType)>>,
    /// Primary-key columns by table name.
    pub primary_keys: HashMap<String, Vec<String>>,
    /// Next id by table name.
    pub restart: HashMap<String, i64>,
    /// Real index root offsets by table name.
    pub index_roots: HashMap<String, Vec<i64>>,
}

/// HSQLDB column domains admitted by the ledger importer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColType {
    /// Integer column.
    Integer,
    /// Text column.
    Varchar,
    /// Date column.
    Date,
    /// Fixed-decimal numeric column.
    Numeric,
}

/// Failure to decode or admit an HSQLDB schema script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchemaError {
    /// The SQL codec rejected syntax outside its bounded HSQLDB grammar.
    Ddl(SqlError),
    /// A codec draft used a storage domain the importer does not support.
    UnsupportedDomain {
        /// Table containing the column.
        table: String,
        /// Column with the unsupported domain.
        column: String,
        /// Normalized SQL storage spelling.
        storage_type: String,
    },
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ddl(error) => write!(
                f,
                "HSQLDB DDL is outside the bounded SQL codec grammar: {error}"
            ),
            Self::UnsupportedDomain {
                table,
                column,
                storage_type,
            } => {
                write!(
                    f,
                    "unsupported HSQLDB domain {storage_type} for {table}.{column}"
                )
            }
        }
    }
}

impl std::error::Error for SchemaError {}

/// Decode with the one SQL grammar owner, then admit storage types against the
/// ledger importer's closed domain catalog.
pub fn parse_script(script: &str) -> Result<OdbSchema, SchemaError> {
    let draft = DdlCodec
        .decode(script, LegacyDdl::Hsqldb)
        .map_err(SchemaError::Ddl)?;
    admit_draft(&draft)
}

fn admit_draft(draft: &SchemaDraft) -> Result<OdbSchema, SchemaError> {
    let mut schema = OdbSchema::default();
    for table in &draft.tables {
        let columns = table
            .columns
            .iter()
            .map(|column| {
                let storage = column.storage_type.as_str();
                let domain = if storage == "INTEGER" {
                    ColType::Integer
                } else if storage.starts_with("VARCHAR")
                    || storage.starts_with("CHAR")
                    || storage == "LONGVARCHAR"
                {
                    ColType::Varchar
                } else if storage == "DATE" {
                    ColType::Date
                } else if storage.starts_with("NUMERIC") || storage.starts_with("DECIMAL") {
                    ColType::Numeric
                } else {
                    return Err(SchemaError::UnsupportedDomain {
                        table: table.name.clone(),
                        column: column.name.clone(),
                        storage_type: storage.to_owned(),
                    });
                };
                Ok((column.name.clone(), domain))
            })
            .collect::<Result<Vec<_>, _>>()?;
        schema.columns.insert(table.name.clone(), columns);
        schema
            .primary_keys
            .insert(table.name.clone(), table.primary_key.clone());
        if let Some(next) = table.restart_with {
            schema.restart.insert(table.name.clone(), next);
        }
        if !table.index_roots.is_empty() {
            schema
                .index_roots
                .insert(table.name.clone(), table.index_roots.clone());
        }
    }
    Ok(schema)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEDGER_DDL: &str = r#"CREATE CACHED TABLE "konto"("k_nr" INTEGER NOT NULL PRIMARY KEY,"k_namn" VARCHAR(50),"k_text" VARCHAR(200),"k_sru_p" INTEGER,"k_sru_m" INTEGER)
CREATE CACHED TABLE "ver"("v_nr" INTEGER NOT NULL PRIMARY KEY,"v_datum" DATE NOT NULL,"v_text" VARCHAR(200))
CREATE CACHED TABLE "trans"("t_nr" INTEGER NOT NULL PRIMARY KEY,"t_ver" INTEGER NOT NULL,"t_konto" INTEGER NOT NULL,"t_belopp" NUMERIC(50,2) NOT NULL,"t_text" VARCHAR(200))
ALTER TABLE "ver" ALTER COLUMN "v_nr" RESTART WITH 11612
ALTER TABLE "trans" ALTER COLUMN "t_nr" RESTART WITH 25471
SET TABLE "trans" INDEX'134576 94648 47888 25471'"#;

    #[test]
    fn admits_the_complete_ledger_fixture_schema() {
        let schema = parse_script(LEDGER_DDL).unwrap();
        assert_eq!(schema.restart["ver"], 11_612);
        assert_eq!(schema.restart["trans"], 25_471);
        assert_eq!(schema.primary_keys["konto"], ["k_nr"]);
        assert_eq!(schema.columns["konto"].len(), 5);
        assert_eq!(schema.index_roots["trans"], [134_576, 94_648, 47_888]);
    }

    #[test]
    fn refuses_unknown_sql_and_unknown_import_domains() {
        assert!(matches!(
            parse_script("DROP TABLE ledger"),
            Err(SchemaError::Ddl(_))
        ));
        assert!(matches!(
            parse_script("CREATE CACHED TABLE t (payload BLOB)"),
            Err(SchemaError::UnsupportedDomain { .. })
        ));
    }
}
