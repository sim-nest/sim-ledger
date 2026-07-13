//! SIM expression projection for ledger reports and queries.

use std::fmt;
use std::num::{ParseIntError, TryFromIntError};

use sim_kernel::{Expr, NumberLiteral, Symbol};

use crate::model::Amount;
use crate::report::{BalanceKey, BalanceRow};

const FIELD_AMOUNT: &str = "amount";
const FIELD_BY: &str = "by";
const FIELD_CODE: &str = "code";
const FIELD_KEY: &str = "key";
const FIELD_KIND: &str = "kind";
const FIELD_ROWS: &str = "rows";
const FIELD_YEARS: &str = "years";

/// A decoded `balances` report call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BalancesCall {
    /// Ledger years to include.
    pub years: Vec<i32>,
    /// When true, group balances by SRU code instead of year-local account.
    pub by_sru: bool,
}

/// Failure while decoding a SIM ledger query expression.
#[derive(Debug)]
pub enum LedgerCodecError {
    /// The root form was not a map.
    ExpectedMap,
    /// A required field is missing.
    MissingField {
        /// Field name.
        field: &'static str,
    },
    /// A field had the wrong expression shape.
    WrongField {
        /// Field name.
        field: &'static str,
        /// Expected shape.
        expected: &'static str,
    },
    /// A symbol carried an unsupported value.
    UnsupportedSymbol {
        /// Field name.
        field: &'static str,
        /// Unsupported symbol text.
        symbol: String,
    },
    /// An integer literal used the wrong number domain.
    UnsupportedNumberDomain {
        /// Field name.
        field: &'static str,
        /// Number domain text.
        domain: String,
    },
    /// An integer literal did not parse.
    InvalidInteger {
        /// Field name.
        field: &'static str,
        /// Source parse error.
        source: ParseIntError,
    },
    /// A year literal did not fit in `i32`.
    YearOutOfRange {
        /// Source conversion error.
        source: TryFromIntError,
    },
}

impl fmt::Display for LedgerCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LedgerCodecError::ExpectedMap => write!(f, "ledger query must be a map"),
            LedgerCodecError::MissingField { field } => {
                write!(f, "ledger query is missing field {field}")
            }
            LedgerCodecError::WrongField { field, expected } => {
                write!(f, "ledger query field {field} must be {expected}")
            }
            LedgerCodecError::UnsupportedSymbol { field, symbol } => {
                write!(f, "unsupported ledger symbol {symbol} in field {field}")
            }
            LedgerCodecError::UnsupportedNumberDomain { field, domain } => {
                write!(
                    f,
                    "unsupported ledger number domain {domain} in field {field}"
                )
            }
            LedgerCodecError::InvalidInteger { field, source } => {
                write!(f, "invalid ledger integer in field {field}: {source}")
            }
            LedgerCodecError::YearOutOfRange { source } => {
                write!(f, "ledger year is out of range: {source}")
            }
        }
    }
}

impl std::error::Error for LedgerCodecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LedgerCodecError::InvalidInteger { source, .. } => Some(source),
            LedgerCodecError::YearOutOfRange { source } => Some(source),
            _ => None,
        }
    }
}

/// Encode balance report rows as a data-position SIM expression.
#[must_use]
pub fn report_to_expr(rows: &[BalanceRow]) -> Expr {
    map(vec![
        entry(FIELD_KIND, Expr::Symbol(ledger_symbol("report"))),
        entry(
            FIELD_ROWS,
            Expr::List(rows.iter().map(balance_row_to_expr).collect()),
        ),
    ])
}

/// Build a SIM expression for a `balances` query.
#[must_use]
pub fn balances_query_expr(years: &[i32], by_sru: bool) -> Expr {
    map(vec![
        entry(FIELD_KIND, Expr::Symbol(ledger_symbol("balances"))),
        entry(
            FIELD_YEARS,
            Expr::List(
                years
                    .iter()
                    .map(|year| integer_expr(i64::from(*year)))
                    .collect(),
            ),
        ),
        entry(
            FIELD_BY,
            Expr::Symbol(ledger_symbol(if by_sru { "sru" } else { "account" })),
        ),
    ])
}

/// Decode a SIM ledger query expression into a `balances` report call.
pub fn balances_call_from_expr(expr: &Expr) -> Result<BalancesCall, LedgerCodecError> {
    let entries = expect_map(expr)?;
    expect_kind(entries, "balances")?;
    let years = expect_list(field(entries, FIELD_YEARS)?, FIELD_YEARS)?
        .iter()
        .map(|expr| {
            let year = read_integer(expr, FIELD_YEARS)?;
            i32::try_from(year).map_err(|source| LedgerCodecError::YearOutOfRange { source })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let by_sru = match expect_symbol(field(entries, FIELD_BY)?, FIELD_BY)?.as_qualified_str() {
        value if value == "ledger/account" => false,
        value if value == "ledger/sru" => true,
        symbol => {
            return Err(LedgerCodecError::UnsupportedSymbol {
                field: FIELD_BY,
                symbol,
            });
        }
    };
    Ok(BalancesCall { years, by_sru })
}

fn balance_row_to_expr(row: &BalanceRow) -> Expr {
    map(vec![
        entry(FIELD_KIND, Expr::Symbol(ledger_symbol("balance-row"))),
        entry(FIELD_KEY, balance_key_to_expr(&row.key)),
        entry(FIELD_AMOUNT, amount_expr(row.amount)),
    ])
}

fn balance_key_to_expr(key: &BalanceKey) -> Expr {
    match key {
        BalanceKey::Account { year, account } => map(vec![
            entry(FIELD_KIND, Expr::Symbol(ledger_symbol("account"))),
            entry("year", integer_expr(i64::from(*year))),
            entry("account", integer_expr(*account)),
        ]),
        BalanceKey::Sru { code } => map(vec![
            entry(FIELD_KIND, Expr::Symbol(ledger_symbol("sru"))),
            entry(FIELD_CODE, integer_expr(i64::from(*code))),
        ]),
    }
}

fn amount_expr(amount: Amount) -> Expr {
    Expr::Number(NumberLiteral {
        domain: ledger_symbol("minor"),
        canonical: amount.0.to_string(),
    })
}

fn integer_expr(value: i64) -> Expr {
    Expr::Number(NumberLiteral {
        domain: ledger_symbol("integer"),
        canonical: value.to_string(),
    })
}

fn map(entries: Vec<(Expr, Expr)>) -> Expr {
    Expr::Map(entries)
}

fn entry(name: &'static str, value: Expr) -> (Expr, Expr) {
    (Expr::Symbol(Symbol::new(name)), value)
}

fn ledger_symbol(name: &'static str) -> Symbol {
    Symbol::qualified("ledger", name)
}

fn expect_map(expr: &Expr) -> Result<&[(Expr, Expr)], LedgerCodecError> {
    match expr {
        Expr::Map(entries) => Ok(entries),
        _ => Err(LedgerCodecError::ExpectedMap),
    }
}

fn expect_kind(entries: &[(Expr, Expr)], expected: &'static str) -> Result<(), LedgerCodecError> {
    let symbol = expect_symbol(field(entries, FIELD_KIND)?, FIELD_KIND)?;
    let expected = ledger_symbol(expected);
    if symbol == &expected {
        Ok(())
    } else {
        Err(LedgerCodecError::UnsupportedSymbol {
            field: FIELD_KIND,
            symbol: symbol.as_qualified_str(),
        })
    }
}

fn field<'a>(
    entries: &'a [(Expr, Expr)],
    name: &'static str,
) -> Result<&'a Expr, LedgerCodecError> {
    entries
        .iter()
        .find_map(|(key, value)| match key {
            Expr::Symbol(symbol) if symbol == &Symbol::new(name) => Some(value),
            _ => None,
        })
        .ok_or(LedgerCodecError::MissingField { field: name })
}

fn expect_list<'a>(expr: &'a Expr, field: &'static str) -> Result<&'a [Expr], LedgerCodecError> {
    match expr {
        Expr::List(items) => Ok(items),
        _ => Err(LedgerCodecError::WrongField {
            field,
            expected: "a list",
        }),
    }
}

fn expect_symbol<'a>(expr: &'a Expr, field: &'static str) -> Result<&'a Symbol, LedgerCodecError> {
    match expr {
        Expr::Symbol(symbol) => Ok(symbol),
        _ => Err(LedgerCodecError::WrongField {
            field,
            expected: "a symbol",
        }),
    }
}

fn read_integer(expr: &Expr, field: &'static str) -> Result<i64, LedgerCodecError> {
    let Expr::Number(NumberLiteral { domain, canonical }) = expr else {
        return Err(LedgerCodecError::WrongField {
            field,
            expected: "an integer number",
        });
    };
    let expected = ledger_symbol("integer");
    if domain != &expected {
        return Err(LedgerCodecError::UnsupportedNumberDomain {
            field,
            domain: domain.as_qualified_str(),
        });
    }
    canonical
        .parse()
        .map_err(|source| LedgerCodecError::InvalidInteger { field, source })
}

#[cfg(test)]
mod tests {
    use sim_codec::{decode_portable, encode_portable};
    use sim_kernel::CodecId;

    use super::*;

    #[test]
    fn report_expr_round_trips_as_portable_data() {
        let expr = report_to_expr(&[
            BalanceRow {
                key: BalanceKey::Account {
                    year: 2024,
                    account: 1910,
                },
                amount: Amount(1_200),
            },
            BalanceRow {
                key: BalanceKey::Sru { code: 3000 },
                amount: Amount(-1_200),
            },
        ]);
        let expected = map(vec![
            entry(FIELD_KIND, Expr::Symbol(ledger_symbol("report"))),
            entry(
                FIELD_ROWS,
                Expr::List(vec![
                    map(vec![
                        entry(FIELD_KIND, Expr::Symbol(ledger_symbol("balance-row"))),
                        entry(
                            FIELD_KEY,
                            map(vec![
                                entry(FIELD_KIND, Expr::Symbol(ledger_symbol("account"))),
                                entry("year", integer_expr(2024)),
                                entry("account", integer_expr(1910)),
                            ]),
                        ),
                        entry(FIELD_AMOUNT, amount_expr(Amount(1_200))),
                    ]),
                    map(vec![
                        entry(FIELD_KIND, Expr::Symbol(ledger_symbol("balance-row"))),
                        entry(
                            FIELD_KEY,
                            map(vec![
                                entry(FIELD_KIND, Expr::Symbol(ledger_symbol("sru"))),
                                entry(FIELD_CODE, integer_expr(3000)),
                            ]),
                        ),
                        entry(FIELD_AMOUNT, amount_expr(Amount(-1_200))),
                    ]),
                ]),
            ),
        ]);
        assert_eq!(expr, expected);

        let text = encode_portable(CodecId(0), &expr).unwrap();
        let decoded = decode_portable(CodecId(0), &text).unwrap();
        assert!(expr.canonical_eq(&decoded));
    }

    #[test]
    fn balances_query_decodes_to_call() {
        let expr = balances_query_expr(&[2022, 2023], true);
        let text = encode_portable(CodecId(0), &expr).unwrap();
        let decoded = decode_portable(CodecId(0), &text).unwrap();

        assert_eq!(
            balances_call_from_expr(&decoded).unwrap(),
            BalancesCall {
                years: vec![2022, 2023],
                by_sru: true,
            }
        );
    }

    #[test]
    fn rejects_unknown_query_kind() {
        let expr = map(vec![entry(
            FIELD_KIND,
            Expr::Symbol(ledger_symbol("other")),
        )]);
        let err = balances_call_from_expr(&expr).unwrap_err();
        assert!(err.to_string().contains("unsupported ledger symbol"));
    }
}
