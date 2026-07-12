//! Ledger records and exact fixed-decimal amount handling.

use std::fmt;

/// Exact money as a signed count of minor units (hundredths). "1234.56" -> 123456.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Amount(pub i64);

impl Amount {
    /// Parse a fixed two-decimal string such as "1234.56", "-7.00", or "42".
    pub fn parse(s: &str) -> Result<Amount, String> {
        if s.is_empty() {
            return Err("amount is empty".to_owned());
        }

        let (negative, unsigned) = match s.strip_prefix('-') {
            Some(rest) => {
                if rest.is_empty() {
                    return Err("amount has no digits".to_owned());
                }
                (true, rest)
            }
            None => (false, s),
        };
        if unsigned.starts_with('+') {
            return Err("amount must not include an explicit plus sign".to_owned());
        }

        let mut parts = unsigned.split('.');
        let whole = parts.next().unwrap_or_default();
        let fraction = parts.next();
        if parts.next().is_some() {
            return Err("amount has more than one decimal point".to_owned());
        }
        if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) {
            return Err("amount whole part must contain digits only".to_owned());
        }

        let whole_units = whole
            .parse::<i64>()
            .map_err(|_| "amount whole part overflows i64".to_owned())?;
        let mut minor = whole_units
            .checked_mul(100)
            .ok_or_else(|| "amount overflows i64 minor units".to_owned())?;

        if let Some(fraction) = fraction {
            if fraction.is_empty() {
                return Err("amount fractional part is empty".to_owned());
            }
            if fraction.len() > 2 {
                return Err("amount has more than two fractional digits".to_owned());
            }
            if !fraction.bytes().all(|b| b.is_ascii_digit()) {
                return Err("amount fractional part must contain digits only".to_owned());
            }

            let cents = match fraction.len() {
                1 => fraction
                    .parse::<i64>()
                    .map_err(|_| "amount fractional part overflows i64".to_owned())?
                    .checked_mul(10)
                    .ok_or_else(|| "amount overflows i64 minor units".to_owned())?,
                2 => fraction
                    .parse::<i64>()
                    .map_err(|_| "amount fractional part overflows i64".to_owned())?,
                _ => 0,
            };
            minor = minor
                .checked_add(cents)
                .ok_or_else(|| "amount overflows i64 minor units".to_owned())?;
        }

        if negative {
            minor = minor
                .checked_neg()
                .ok_or_else(|| "amount overflows i64 minor units".to_owned())?;
        }
        Ok(Amount(minor))
    }

    /// Format this amount with exactly two fractional digits.
    #[must_use]
    pub fn to_decimal_string(self) -> String {
        let value = self.0 as i128;
        let (sign, magnitude) = if value < 0 {
            ("-", -value)
        } else {
            ("", value)
        };
        format!("{sign}{}.{:02}", magnitude / 100, magnitude % 100)
    }
}

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_decimal_string())
    }
}

/// A year-local account (the "konto" row). Numbers are NOT stable across years;
/// SRU codes bridge years for reporting.
#[derive(Clone, Debug, PartialEq)]
pub struct Account {
    /// Account number from the source year.
    pub number: i64,
    /// Account name.
    pub name: String,
    /// Optional account note.
    pub note: Option<String>,
    /// SRU code used for positive balances.
    pub sru_plus: Option<i32>,
    /// SRU code used for negative balances.
    pub sru_minus: Option<i32>,
}

/// A voucher / verifikation (the "ver" row).
#[derive(Clone, Debug, PartialEq)]
pub struct Voucher {
    /// Canonical id, monotonic across years.
    pub id: i64,
    /// Original voucher id, kept for audit.
    pub source_id: Option<i64>,
    /// ISO-8601 date string.
    pub date: String,
    /// Optional voucher text.
    pub text: Option<String>,
}

/// A posting / transaction line (the "trans" row).
#[derive(Clone, Debug, PartialEq)]
pub struct Posting {
    /// Canonical id, monotonic across years.
    pub id: i64,
    /// Original posting id, kept for audit.
    pub source_id: Option<i64>,
    /// Voucher id this posting belongs to.
    pub voucher_id: i64,
    /// Year-local account number.
    pub account: i64,
    /// Signed posting amount.
    pub amount: Amount,
    /// Optional posting text.
    pub text: Option<String>,
}

/// A whole year, ready to persist or imported for further processing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct YearData {
    /// Ledger year.
    pub year: i32,
    /// Year-local accounts.
    pub accounts: Vec<Account>,
    /// Vouchers for the year.
    pub vouchers: Vec<Voucher>,
    /// Posting lines for the year.
    pub postings: Vec<Posting>,
}

/// Double-entry: a voucher's postings must sum to exactly zero.
#[must_use]
pub fn is_balanced(postings: &[Posting]) -> bool {
    postings.iter().map(|p| p.amount.0 as i128).sum::<i128>() == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixed_decimal_amounts() {
        assert_eq!(Amount::parse("1234.56").unwrap().0, 123_456);
        assert_eq!(Amount::parse("-7.00").unwrap().0, -700);
        assert_eq!(Amount::parse("42").unwrap().0, 4_200);
    }

    #[test]
    fn rejects_too_many_fraction_digits() {
        assert!(Amount::parse("1.234").is_err());
    }

    #[test]
    fn formats_amounts_for_round_trip() {
        for text in ["0.00", "1234.56", "-7.00", "42.10"] {
            let amount = Amount::parse(text).unwrap();
            assert_eq!(amount.to_decimal_string(), text);
            assert_eq!(Amount::parse(&amount.to_decimal_string()).unwrap(), amount);
            assert_eq!(amount.to_string(), text);
        }
    }

    #[test]
    fn checks_double_entry_balance() {
        let balanced = vec![posting(100), posting(-100)];
        let unbalanced = vec![posting(100), posting(-99)];

        assert!(is_balanced(&balanced));
        assert!(!is_balanced(&unbalanced));
    }

    fn posting(amount: i64) -> Posting {
        Posting {
            id: amount,
            source_id: None,
            voucher_id: 1,
            account: 1910,
            amount: Amount(amount),
            text: None,
        }
    }
}
