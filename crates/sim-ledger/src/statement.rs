//! Declarative admission of bank statement rows into reconciliation snapshots.
//!
//! A [`StatementProfile`] is data: adding an institution or export layout does
//! not add a parser implementation. Admission is fail-closed per row and the
//! returned snapshot contains accepted rows only.

use std::collections::BTreeSet;
use std::fmt;

use crate::Amount;

/// Current serialized statement-profile contract version.
pub const STATEMENT_PROFILE_VERSION: u16 = 1;

/// Current serialized canonical-row contract version.
///
/// Readers must refuse unknown versions and migrate older persisted rows
/// explicitly; admission never guesses at a historical representation.
pub const CANONICAL_STATEMENT_ROW_VERSION: u16 = 1;

/// Supported date renderings in statement data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum DateFormat {
    /// `YYYY-MM-DD`.
    Iso8601,
    /// `DD/MM/YYYY`.
    DayMonthYearSlash,
}

/// Declarative placement and sign convention for an amount.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum AmountLayout {
    /// One signed column. A leading minus is allowed; plus and parentheses are refused.
    Signed {
        /// Zero-based column index.
        column: usize,
    },
    /// Separate unsigned debit and credit columns; exactly one must be populated.
    DebitCredit {
        /// Debit column, admitted as a negative amount.
        debit_column: usize,
        /// Credit column, admitted as a positive amount.
        credit_column: usize,
    },
}

/// Versioned, serializable statement layout. Institution-specific behavior belongs here.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct StatementProfile {
    /// Contract version. Only [`STATEMENT_PROFILE_VERSION`] is admitted.
    pub version: u16,
    /// Stable profile identity recorded on every canonical row.
    pub id: String,
    /// One-byte field delimiter.
    pub delimiter: u8,
    /// Whether the first non-empty record is a header.
    pub has_header: bool,
    /// Date column.
    pub date_column: usize,
    /// Accepted date rendering.
    pub date_format: DateFormat,
    /// Amount columns and sign rule.
    pub amount: AmountLayout,
    /// Decimal separator declared by this source.
    pub decimal_separator: u8,
    /// Exact fractional precision declared by this source (zero through two).
    pub fractional_digits: u8,
    /// The sole currency admitted from this source.
    pub currency: String,
    /// Optional currency column. When present every row must equal `currency`.
    pub currency_column: Option<usize>,
    /// Source-row identity column, used to reject duplicates.
    pub identity_column: usize,
    /// Optional description column.
    pub description_column: Option<usize>,
}

/// One exact ledger balance frozen alongside a statement.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct LedgerBalanceAtCutoff {
    /// Ledger account identity.
    pub account: String,
    /// Exact balance at the cutoff.
    pub amount: Amount,
}

/// Canonical admitted statement value with complete source provenance.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct CanonicalStatementRow {
    /// Serialized row contract version.
    pub version: u16,
    /// Canonical ISO-8601 date.
    pub date: String,
    /// Exact signed minor-unit amount.
    pub amount: Amount,
    /// Declared ISO-style currency code.
    pub currency: String,
    /// Optional source description.
    pub description: Option<String>,
    /// Identity of this row in the source export.
    pub source_identity: String,
    /// Opaque source/export reference.
    pub source_ref: String,
    /// One-based physical record ordinal.
    pub ordinal: usize,
    /// Profile identity used for admission.
    pub profile_id: String,
    /// Original record bytes, excluding the line terminator.
    pub original_bytes: Vec<u8>,
}

/// A rejected source record and its typed refusal.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct RejectedStatementRow {
    /// One-based physical record ordinal.
    pub ordinal: usize,
    /// Original record bytes, excluding the line terminator.
    pub original_bytes: Vec<u8>,
    /// Exact refusal reason.
    pub reason: StatementError,
}

/// Immutable reconciliation inputs at an explicit cutoff.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct StatementSnapshot {
    /// Caller-defined cutoff, conventionally an ISO-8601 instant or date.
    pub cutoff: String,
    /// Opaque source/export reference.
    pub source_ref: String,
    /// Profile identity.
    pub profile_id: String,
    /// Canonical rows accepted before the cutoff.
    pub rows: Vec<CanonicalStatementRow>,
    /// Checked sum of all accepted rows.
    pub total: Amount,
    /// Ledger balances frozen at the same cutoff.
    pub ledger_balances: Vec<LedgerBalanceAtCutoff>,
}

/// Complete admission outcome. Rejections never enter [`StatementSnapshot::rows`].
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct StatementAdmission {
    /// Atomic snapshot of accepted values and ledger comparison inputs.
    pub snapshot: StatementSnapshot,
    /// Refused rows retained for review.
    pub rejected: Vec<RejectedStatementRow>,
}

/// Typed profile or row refusal.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum StatementError {
    /// The profile requires an explicit migration before it can be used.
    UnsupportedProfileVersion { found: u16, supported: u16 },
    /// A profile declaration is invalid.
    InvalidProfile(String),
    /// Input was not UTF-8.
    InvalidEncoding,
    /// A row did not contain every declared column.
    MissingColumn { column: usize },
    /// A required date was empty or invalid.
    MissingOrInvalidDate,
    /// A source identity was empty.
    MissingSourceIdentity,
    /// A source identity occurred more than once.
    DuplicateSourceIdentity(String),
    /// The row's currency differs from the profile's one declared currency.
    MixedCurrency { expected: String, found: String },
    /// The amount used more precision than declared or supported.
    ExcessPrecision,
    /// The amount sign could not be interpreted unambiguously.
    AmbiguousSign,
    /// The amount was otherwise malformed.
    MalformedAmount,
    /// Exact minor-unit conversion or snapshot addition overflowed.
    ArithmeticOverflow,
    /// A delimited row was malformed.
    MalformedRow,
}

impl fmt::Display for StatementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for StatementError {}

/// Admit delimited statement bytes through profile data and freeze ledger inputs.
///
/// Profile-level errors refuse the operation. Row-level errors are returned in
/// [`StatementAdmission::rejected`] and cannot affect the accepted snapshot.
pub fn admit_statement(
    profile: &StatementProfile,
    source_ref: &str,
    cutoff: &str,
    input: &[u8],
    ledger_balances: Vec<LedgerBalanceAtCutoff>,
) -> Result<StatementAdmission, StatementError> {
    validate_profile(profile)?;
    if source_ref.is_empty() || cutoff.is_empty() {
        return Err(StatementError::InvalidProfile(
            "source_ref and cutoff must be non-empty".to_owned(),
        ));
    }
    let text = std::str::from_utf8(input).map_err(|_| StatementError::InvalidEncoding)?;
    let mut rows = Vec::new();
    let mut rejected = Vec::new();
    let mut identities = BTreeSet::new();
    let mut total = 0_i64;

    let mut header_pending = profile.has_header;
    for (line_index, raw_line) in text.lines().enumerate() {
        let ordinal = line_index + 1;
        let raw = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if raw.is_empty() {
            continue;
        }
        if header_pending {
            header_pending = false;
            continue;
        }
        let original_bytes = raw.as_bytes().to_vec();
        match parse_row(profile, source_ref, ordinal, raw, &mut identities) {
            Ok(row) => match total.checked_add(row.amount.0) {
                Some(next) => {
                    total = next;
                    rows.push(row);
                }
                None => rejected.push(RejectedStatementRow {
                    ordinal,
                    original_bytes,
                    reason: StatementError::ArithmeticOverflow,
                }),
            },
            Err(reason) => rejected.push(RejectedStatementRow {
                ordinal,
                original_bytes,
                reason,
            }),
        }
    }

    Ok(StatementAdmission {
        snapshot: StatementSnapshot {
            cutoff: cutoff.to_owned(),
            source_ref: source_ref.to_owned(),
            profile_id: profile.id.clone(),
            rows,
            total: Amount(total),
            ledger_balances,
        },
        rejected,
    })
}

fn validate_profile(profile: &StatementProfile) -> Result<(), StatementError> {
    if profile.version != STATEMENT_PROFILE_VERSION {
        return Err(StatementError::UnsupportedProfileVersion {
            found: profile.version,
            supported: STATEMENT_PROFILE_VERSION,
        });
    }
    if profile.id.is_empty() || profile.currency.is_empty() {
        return Err(StatementError::InvalidProfile(
            "profile id and currency must be non-empty".to_owned(),
        ));
    }
    if matches!(profile.delimiter, b'\n' | b'\r' | b'"') || !profile.delimiter.is_ascii() {
        return Err(StatementError::InvalidProfile(
            "delimiter must be a safe single ASCII byte".to_owned(),
        ));
    }
    if !matches!(profile.decimal_separator, b'.' | b',') || profile.fractional_digits > 2 {
        return Err(StatementError::InvalidProfile(
            "decimal rule must declare separator and zero through two digits".to_owned(),
        ));
    }
    Ok(())
}

fn parse_row(
    profile: &StatementProfile,
    source_ref: &str,
    ordinal: usize,
    raw: &str,
    identities: &mut BTreeSet<String>,
) -> Result<CanonicalStatementRow, StatementError> {
    let fields = split_fields(raw, profile.delimiter as char)?;
    let field = |column: usize| {
        fields
            .get(column)
            .map(|value| value.trim())
            .ok_or(StatementError::MissingColumn { column })
    };
    let source_identity = field(profile.identity_column)?.to_owned();
    if source_identity.is_empty() {
        return Err(StatementError::MissingSourceIdentity);
    }
    if identities.contains(&source_identity) {
        return Err(StatementError::DuplicateSourceIdentity(source_identity));
    }
    let date = canonical_date(field(profile.date_column)?, profile.date_format)?;
    if let Some(column) = profile.currency_column {
        let found = field(column)?;
        if found != profile.currency {
            return Err(StatementError::MixedCurrency {
                expected: profile.currency.clone(),
                found: found.to_owned(),
            });
        }
    }
    let amount = match profile.amount {
        AmountLayout::Signed { column } => parse_amount(field(column)?, profile, false)?,
        AmountLayout::DebitCredit {
            debit_column,
            credit_column,
        } => {
            let debit = field(debit_column)?;
            let credit = field(credit_column)?;
            match (debit.is_empty(), credit.is_empty()) {
                (false, true) => parse_amount(debit, profile, true)?,
                (true, false) => parse_amount(credit, profile, false)?,
                _ => return Err(StatementError::AmbiguousSign),
            }
        }
    };
    let row = CanonicalStatementRow {
        version: CANONICAL_STATEMENT_ROW_VERSION,
        date,
        amount,
        currency: profile.currency.clone(),
        description: profile
            .description_column
            .map(field)
            .transpose()?
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
        source_identity,
        source_ref: source_ref.to_owned(),
        ordinal,
        profile_id: profile.id.clone(),
        original_bytes: raw.as_bytes().to_vec(),
    };
    identities.insert(row.source_identity.clone());
    Ok(row)
}

fn split_fields(line: &str, delimiter: char) -> Result<Vec<String>, StatementError> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();
    let mut quoted = false;
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => {
                current.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            value if value == delimiter && !quoted => {
                fields.push(std::mem::take(&mut current));
            }
            value => current.push(value),
        }
    }
    if quoted {
        return Err(StatementError::MalformedRow);
    }
    fields.push(current);
    Ok(fields)
}

fn parse_amount(
    text: &str,
    profile: &StatementProfile,
    negate: bool,
) -> Result<Amount, StatementError> {
    if text.is_empty() || text.starts_with('+') || text.contains(['(', ')']) {
        return Err(StatementError::AmbiguousSign);
    }
    if negate && text.starts_with('-') {
        return Err(StatementError::AmbiguousSign);
    }
    let separator = profile.decimal_separator as char;
    let (negative, unsigned) = text
        .strip_prefix('-')
        .map_or((false, text), |rest| (true, rest));
    let mut parts = unsigned.split(separator);
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next();
    if parts.next().is_some() || whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) {
        return Err(StatementError::MalformedAmount);
    }
    let fraction = fraction.unwrap_or("");
    if fraction.len() > profile.fractional_digits as usize {
        return Err(StatementError::ExcessPrecision);
    }
    if !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return Err(StatementError::MalformedAmount);
    }
    let scale = 10_i64
        .checked_pow(u32::from(profile.fractional_digits))
        .ok_or(StatementError::ArithmeticOverflow)?;
    let whole = whole
        .parse::<i64>()
        .map_err(|_| StatementError::ArithmeticOverflow)?;
    let mut value = whole
        .checked_mul(scale)
        .ok_or(StatementError::ArithmeticOverflow)?;
    if !fraction.is_empty() {
        let parsed = fraction
            .parse::<i64>()
            .map_err(|_| StatementError::MalformedAmount)?;
        let padding = 10_i64.pow(u32::from(profile.fractional_digits) - fraction.len() as u32);
        value = value
            .checked_add(
                parsed
                    .checked_mul(padding)
                    .ok_or(StatementError::ArithmeticOverflow)?,
            )
            .ok_or(StatementError::ArithmeticOverflow)?;
    }
    if profile.fractional_digits < 2 {
        value = value
            .checked_mul(10_i64.pow(u32::from(2 - profile.fractional_digits)))
            .ok_or(StatementError::ArithmeticOverflow)?;
    }
    if negative ^ negate {
        value = value
            .checked_neg()
            .ok_or(StatementError::ArithmeticOverflow)?;
    }
    Ok(Amount(value))
}

fn canonical_date(text: &str, format: DateFormat) -> Result<String, StatementError> {
    if text.is_empty() {
        return Err(StatementError::MissingOrInvalidDate);
    }
    let (year, month, day) = match format {
        DateFormat::Iso8601 => date_parts(text, '-')?,
        DateFormat::DayMonthYearSlash => {
            let (day, month, year) = date_parts(text, '/')?;
            (year, month, day)
        }
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return Err(StatementError::MissingOrInvalidDate),
    };
    if year == 0 || day == 0 || day > max_day {
        return Err(StatementError::MissingOrInvalidDate);
    }
    Ok(format!("{year:04}-{month:02}-{day:02}"))
}

fn date_parts(text: &str, separator: char) -> Result<(u32, u32, u32), StatementError> {
    let parts = text
        .split(separator)
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| StatementError::MissingOrInvalidDate)?;
    if parts.len() != 3 {
        return Err(StatementError::MissingOrInvalidDate);
    }
    Ok((parts[0], parts[1], parts[2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signed_profile() -> StatementProfile {
        StatementProfile {
            version: 1,
            id: "synthetic-pipe-v1".to_owned(),
            delimiter: b'|',
            has_header: true,
            date_column: 1,
            date_format: DateFormat::Iso8601,
            amount: AmountLayout::Signed { column: 2 },
            decimal_separator: b'.',
            fractional_digits: 2,
            currency: "SEK".to_owned(),
            currency_column: Some(3),
            identity_column: 0,
            description_column: Some(4),
        }
    }

    fn split_profile() -> StatementProfile {
        StatementProfile {
            version: 1,
            id: "synthetic-semicolon-v1".to_owned(),
            delimiter: b';',
            has_header: false,
            date_column: 0,
            date_format: DateFormat::DayMonthYearSlash,
            amount: AmountLayout::DebitCredit {
                debit_column: 2,
                credit_column: 3,
            },
            decimal_separator: b',',
            fractional_digits: 2,
            currency: "SEK".to_owned(),
            currency_column: None,
            identity_column: 1,
            description_column: Some(4),
        }
    }

    #[test]
    fn two_formats_are_profile_data_and_produce_identical_canonical_values() {
        let ledger = vec![LedgerBalanceAtCutoff {
            account: "bank".to_owned(),
            amount: Amount(4_200),
        }];
        let pipe = admit_statement(
            &signed_profile(),
            "export-a",
            "2026-08-25",
            b"id|date|amount|currency|text\na1|2026-08-24|-12.50|SEK|Coffee\na2|2026-08-25|54.50|SEK|Invoice\n",
            ledger.clone(),
        )
        .unwrap();
        let semi = admit_statement(
            &split_profile(),
            "export-b",
            "2026-08-25",
            b"24/08/2026;b1;12,50;;Coffee\n25/08/2026;b2;;54,50;Invoice\n",
            ledger,
        )
        .unwrap();
        let values = |admission: &StatementAdmission| {
            admission
                .snapshot
                .rows
                .iter()
                .map(|row| (row.date.clone(), row.amount, row.description.clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(values(&pipe), values(&semi));
        assert_eq!(pipe.snapshot.total, Amount(4_200));
        assert_eq!(pipe.snapshot.cutoff, semi.snapshot.cutoff);
        assert_eq!(pipe.snapshot.ledger_balances, semi.snapshot.ledger_balances);
        assert_eq!(
            pipe.snapshot.rows[0].original_bytes,
            b"a1|2026-08-24|-12.50|SEK|Coffee"
        );
        assert_eq!(pipe.snapshot.rows[0].source_ref, "export-a");
        assert_eq!(pipe.snapshot.rows[0].ordinal, 2);
    }

    #[test]
    fn every_bad_row_is_typed_and_absent_from_snapshot() {
        let input = b"id|date|amount|currency|text\n\
ok|2026-08-20|1.00|SEK|accepted\n\
fx|2026-08-21|2.00|EUR|mixed\n\
precise|2026-08-21|1.001|SEK|precision\n\
sign|2026-08-21|+1.00|SEK|sign\n\
date||1.00|SEK|date\n\
ok|2026-08-22|1.00|SEK|duplicate\n\
broken|2026-08-22|wat|SEK|malformed\n\
overflow|2026-08-22|92233720368547758.08|SEK|overflow\n";
        let admitted = admit_statement(
            &signed_profile(),
            "reject-fixture",
            "2026-08-25",
            input,
            Vec::new(),
        )
        .unwrap();
        assert_eq!(admitted.snapshot.rows.len(), 1);
        assert_eq!(admitted.snapshot.rows[0].source_identity, "ok");
        assert_eq!(admitted.rejected.len(), 7);
        assert!(matches!(
            admitted.rejected[0].reason,
            StatementError::MixedCurrency { .. }
        ));
        assert_eq!(admitted.rejected[1].reason, StatementError::ExcessPrecision);
        assert_eq!(admitted.rejected[2].reason, StatementError::AmbiguousSign);
        assert_eq!(
            admitted.rejected[3].reason,
            StatementError::MissingOrInvalidDate
        );
        assert!(matches!(
            admitted.rejected[4].reason,
            StatementError::DuplicateSourceIdentity(_)
        ));
        assert_eq!(admitted.rejected[5].reason, StatementError::MalformedAmount);
        assert_eq!(
            admitted.rejected[6].reason,
            StatementError::ArithmeticOverflow
        );
    }

    #[test]
    fn split_amount_refuses_ambiguous_sign_and_profile_requires_migration() {
        let admitted = admit_statement(
            &split_profile(),
            "ambiguous",
            "2026-08-25",
            b"25/08/2026;x;1,00;2,00;both\n25/08/2026;y;;;neither\n",
            Vec::new(),
        )
        .unwrap();
        assert!(admitted.snapshot.rows.is_empty());
        assert!(
            admitted
                .rejected
                .iter()
                .all(|row| row.reason == StatementError::AmbiguousSign)
        );

        let mut future = signed_profile();
        future.version = 2;
        assert_eq!(
            admit_statement(&future, "future", "2026-08-25", b"", Vec::new()).unwrap_err(),
            StatementError::UnsupportedProfileVersion {
                found: 2,
                supported: 1
            }
        );
    }

    #[test]
    fn snapshot_sum_overflow_refuses_only_the_overflowing_row() {
        let input = b"id|date|amount|currency|text\na|2026-08-25|92233720368547758.07|SEK|max\nb|2026-08-25|0.01|SEK|overflow\n";
        let admitted = admit_statement(
            &signed_profile(),
            "sum-overflow",
            "2026-08-25",
            input,
            Vec::new(),
        )
        .unwrap();
        assert_eq!(admitted.snapshot.rows.len(), 1);
        assert_eq!(admitted.snapshot.total, Amount(i64::MAX));
        assert_eq!(
            admitted.rejected[0].reason,
            StatementError::ArithmeticOverflow
        );
    }

    #[test]
    fn rejected_row_does_not_reserve_identity_and_header_is_first_non_empty_record() {
        let input = b"\nid|date|amount|currency|text\nreuse||1.00|SEK|bad\nreuse|2026-08-25|1.00|SEK|good\n";
        let admitted = admit_statement(
            &signed_profile(),
            "retry-identity",
            "2026-08-25",
            input,
            Vec::new(),
        )
        .unwrap();
        assert_eq!(admitted.snapshot.rows.len(), 1);
        assert_eq!(
            admitted.snapshot.rows[0].version,
            CANONICAL_STATEMENT_ROW_VERSION
        );
        assert_eq!(admitted.snapshot.rows[0].source_identity, "reuse");
        assert_eq!(admitted.snapshot.rows[0].ordinal, 4);
        assert_eq!(admitted.rejected.len(), 1);
        assert_eq!(
            admitted.rejected[0].reason,
            StatementError::MissingOrInvalidDate
        );
    }
}
