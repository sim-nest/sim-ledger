//! HSQLDB script metadata parsing.

use std::collections::HashMap;

/// Parsed table layout and id high-water marks from `database/script`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OdbSchema {
    /// Column layout by table name.
    pub columns: HashMap<String, Vec<(String, ColType)>>,
    /// Next id by table name.
    pub restart: HashMap<String, i64>,
    /// Index root offsets by table name.
    pub index_roots: HashMap<String, Vec<i64>>,
}

/// HSQLDB column types used by the ledger tables.
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

/// Parse HSQLDB table layouts, index roots, and `RESTART WITH` id counters.
#[must_use]
pub fn parse_script(script: &str) -> OdbSchema {
    let mut schema = OdbSchema::default();
    for line in script
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some((table, columns)) = parse_create_table(line) {
            schema.columns.insert(table, columns);
        }
        if let Some((table, next)) = parse_restart(line) {
            schema.restart.insert(table, next);
        }
        if let Some((table, roots)) = parse_index_roots(line) {
            schema.index_roots.insert(table, roots);
        }
    }
    schema
}

fn parse_create_table(line: &str) -> Option<(String, Vec<(String, ColType)>)> {
    let upper = line.to_ascii_uppercase();
    let table_pos = upper.find(" TABLE ")?;
    let after_table = line[table_pos + " TABLE ".len()..].trim_start();
    let (table, rest) = parse_quoted(after_table)?;
    let open = rest.find('(')?;
    let close = rest.rfind(')')?;
    let body = &rest[open + 1..close];
    let columns = split_top_level_commas(body)
        .into_iter()
        .filter_map(parse_column)
        .collect();
    Some((table, columns))
}

fn parse_column(part: &str) -> Option<(String, ColType)> {
    let (name, rest) = parse_quoted(part.trim())?;
    let ty = rest.trim_start().to_ascii_uppercase();
    let col_type = if ty.starts_with("INTEGER") {
        ColType::Integer
    } else if ty.starts_with("VARCHAR") || ty.starts_with("CHAR") || ty.starts_with("LONGVARCHAR") {
        ColType::Varchar
    } else if ty.starts_with("DATE") {
        ColType::Date
    } else if ty.starts_with("NUMERIC") || ty.starts_with("DECIMAL") {
        ColType::Numeric
    } else {
        return None;
    };
    Some((name, col_type))
}

fn parse_restart(line: &str) -> Option<(String, i64)> {
    let upper = line.to_ascii_uppercase();
    if !upper.starts_with("ALTER TABLE ") {
        return None;
    }
    let (table, _) = parse_quoted(line["ALTER TABLE ".len()..].trim_start())?;
    let restart_pos = upper.find(" RESTART WITH ")? + " RESTART WITH ".len();
    let next = line[restart_pos..]
        .split_whitespace()
        .next()?
        .trim_end_matches(';')
        .parse()
        .ok()?;
    Some((table, next))
}

fn parse_index_roots(line: &str) -> Option<(String, Vec<i64>)> {
    let upper = line.to_ascii_uppercase();
    if !upper.starts_with("SET TABLE ") {
        return None;
    }
    let (table, rest) = parse_quoted(line["SET TABLE ".len()..].trim_start())?;
    let index_pos = rest.to_ascii_uppercase().find(" INDEX'")? + " INDEX'".len();
    let roots_text = &rest[index_pos..];
    let end = roots_text.find('\'')?;
    let roots = roots_text[..end]
        .split_whitespace()
        .map(str::parse)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    Some((table, roots))
}

fn parse_quoted(input: &str) -> Option<(String, &str)> {
    let input = input.strip_prefix('"')?;
    let end = input.find('"')?;
    Some((input[..end].to_owned(), &input[end + 1..]))
}

fn split_top_level_commas(input: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0_i32;
    for (index, byte) in input.bytes().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b',' if depth == 0 => {
                parts.push(input[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(input[start..].trim());
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_ledger_table_lines() {
        let schema = parse_script(
            r#"
CREATE CACHED TABLE "konto"("k_nr" INTEGER NOT NULL PRIMARY KEY,"k_namn" VARCHAR(50),"k_text" VARCHAR(200),"k_sru_p" INTEGER,"k_sru_m" INTEGER)
CREATE CACHED TABLE "ver"("v_nr" INTEGER NOT NULL PRIMARY KEY,"v_datum" DATE NOT NULL,"v_text" VARCHAR(200))
CREATE CACHED TABLE "trans"("t_nr" INTEGER NOT NULL PRIMARY KEY,"t_ver" INTEGER NOT NULL,"t_konto" INTEGER NOT NULL,"t_belopp" NUMERIC(50,2) NOT NULL,"t_text" VARCHAR(200))
ALTER TABLE "ver" ALTER COLUMN "v_nr" RESTART WITH 11612
ALTER TABLE "trans" ALTER COLUMN "t_nr" RESTART WITH 25471
SET TABLE "trans" INDEX'134576 94648 47888 25471'
"#,
        );

        assert_eq!(schema.restart["ver"], 11_612);
        assert_eq!(schema.restart["trans"], 25_471);
        assert_eq!(
            schema.columns["konto"],
            vec![
                ("k_nr".to_owned(), ColType::Integer),
                ("k_namn".to_owned(), ColType::Varchar),
                ("k_text".to_owned(), ColType::Varchar),
                ("k_sru_p".to_owned(), ColType::Integer),
                ("k_sru_m".to_owned(), ColType::Integer),
            ]
        );
        assert_eq!(
            schema.index_roots["trans"],
            vec![134_576, 94_648, 47_888, 25_471]
        );
    }
}
