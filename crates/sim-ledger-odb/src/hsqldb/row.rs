//! HSQLDB 1.8 column encoding.

use std::fmt;
use std::str;

use time::{Date, Month, OffsetDateTime};

use crate::ColType;

const NULL_MARKER: u8 = 0;
const PRESENT_MARKER: u8 = 1;
const MILLIS_PER_SECOND: i64 = 1_000;

/// A decoded HSQLDB column value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cell {
    /// SQL null.
    Null,
    /// Integer value.
    Int(i64),
    /// Text value.
    Str(String),
    /// ISO `YYYY-MM-DD` date value.
    Date(String),
    /// Fixed-decimal numeric value in minor units.
    Num(i64),
}

/// Failure while reading an HSQLDB column value.
#[derive(Debug)]
pub struct HsqlError {
    kind: HsqlErrorKind,
}

#[derive(Debug)]
enum HsqlErrorKind {
    UnexpectedEof {
        pos: usize,
        needed: usize,
        len: usize,
    },
    InvalidUtf8(str::Utf8Error),
    InvalidDateMillis(i64),
    NegativeLength(i32),
    UnsupportedNumericScale(i32),
    InvalidBigInteger,
    NumericOverflow,
    InvalidRowOffset(i64),
    InvalidRowSize(i32),
    RowCycle(i64),
}

impl HsqlError {
    pub(crate) fn unexpected_eof(pos: usize, needed: usize, len: usize) -> HsqlError {
        HsqlError {
            kind: HsqlErrorKind::UnexpectedEof { pos, needed, len },
        }
    }

    fn invalid_utf8(source: str::Utf8Error) -> HsqlError {
        HsqlError {
            kind: HsqlErrorKind::InvalidUtf8(source),
        }
    }

    fn invalid_date_millis(millis: i64) -> HsqlError {
        HsqlError {
            kind: HsqlErrorKind::InvalidDateMillis(millis),
        }
    }

    fn negative_length(len: i32) -> HsqlError {
        HsqlError {
            kind: HsqlErrorKind::NegativeLength(len),
        }
    }

    fn unsupported_numeric_scale(scale: i32) -> HsqlError {
        HsqlError {
            kind: HsqlErrorKind::UnsupportedNumericScale(scale),
        }
    }

    fn invalid_big_integer() -> HsqlError {
        HsqlError {
            kind: HsqlErrorKind::InvalidBigInteger,
        }
    }

    pub(crate) fn numeric_overflow() -> HsqlError {
        HsqlError {
            kind: HsqlErrorKind::NumericOverflow,
        }
    }

    pub(crate) fn invalid_row_offset(offset: i64) -> HsqlError {
        HsqlError {
            kind: HsqlErrorKind::InvalidRowOffset(offset),
        }
    }

    pub(crate) fn invalid_row_size(size: i32) -> HsqlError {
        HsqlError {
            kind: HsqlErrorKind::InvalidRowSize(size),
        }
    }

    pub(crate) fn row_cycle(offset: i64) -> HsqlError {
        HsqlError {
            kind: HsqlErrorKind::RowCycle(offset),
        }
    }
}

impl fmt::Display for HsqlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            HsqlErrorKind::UnexpectedEof { pos, needed, len } => write!(
                f,
                "unexpected end of HSQLDB row at byte {pos}; need {needed} byte(s), buffer has {len}"
            ),
            HsqlErrorKind::InvalidUtf8(source) => {
                write!(f, "invalid UTF-8 in HSQLDB string column: {source}")
            }
            HsqlErrorKind::InvalidDateMillis(millis) => {
                write!(f, "invalid HSQLDB DATE epoch millis value {millis}")
            }
            HsqlErrorKind::NegativeLength(len) => {
                write!(f, "negative HSQLDB byte length {len}")
            }
            HsqlErrorKind::UnsupportedNumericScale(scale) => {
                write!(f, "unsupported HSQLDB NUMERIC scale {scale}")
            }
            HsqlErrorKind::InvalidBigInteger => {
                write!(f, "invalid HSQLDB BigInteger payload")
            }
            HsqlErrorKind::NumericOverflow => {
                write!(f, "HSQLDB NUMERIC value does not fit in minor units")
            }
            HsqlErrorKind::InvalidRowOffset(offset) => {
                write!(f, "invalid HSQLDB row offset {offset}")
            }
            HsqlErrorKind::InvalidRowSize(size) => {
                write!(f, "invalid HSQLDB row block size {size}")
            }
            HsqlErrorKind::RowCycle(offset) => {
                write!(
                    f,
                    "cycle while following HSQLDB row index at offset {offset}"
                )
            }
        }
    }
}

impl std::error::Error for HsqlError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            HsqlErrorKind::InvalidUtf8(source) => Some(source),
            _ => None,
        }
    }
}

/// Decode one column value at `pos`, returning the cell and the next position.
pub fn read_cell(buf: &[u8], pos: usize, ty: ColType) -> Result<(Cell, usize), HsqlError> {
    let (marker, pos) = read_u8(buf, pos)?;
    if marker == NULL_MARKER {
        return Ok((Cell::Null, pos));
    }

    match ty {
        ColType::Integer => {
            let (value, pos) = read_i32(buf, pos)?;
            Ok((Cell::Int(i64::from(value)), pos))
        }
        ColType::Varchar => {
            let (bytes, pos) = read_len_bytes(buf, pos)?;
            let value = str::from_utf8(bytes)
                .map_err(HsqlError::invalid_utf8)?
                .to_owned();
            Ok((Cell::Str(value), pos))
        }
        ColType::Date => {
            let (millis, pos) = read_i64(buf, pos)?;
            Ok((Cell::Date(date_from_millis(millis)?), pos))
        }
        ColType::Numeric => {
            let (scale, pos) = read_i32(buf, pos)?;
            let (bytes, pos) = read_len_bytes(buf, pos)?;
            let unscaled = decode_big_integer(bytes)?;
            Ok((Cell::Num(to_minor_units(unscaled, scale)?), pos))
        }
    }
}

/// Encode one column value using the HSQLDB type encoding.
///
/// Panics when a non-null cell does not match `ty`, when a date is not
/// `YYYY-MM-DD`, or when an encoded length does not fit HSQLDB's i32 length.
pub fn write_cell(out: &mut Vec<u8>, cell: &Cell, ty: ColType) {
    if matches!(cell, Cell::Null) {
        out.push(NULL_MARKER);
        return;
    }

    out.push(PRESENT_MARKER);
    match (cell, ty) {
        (Cell::Int(value), ColType::Integer) => write_i32(out, checked_i32(*value)),
        (Cell::Str(value), ColType::Varchar) => write_len_bytes(out, value.as_bytes()),
        (Cell::Date(value), ColType::Date) => write_i64(out, millis_from_date(value)),
        (Cell::Num(value), ColType::Numeric) => {
            write_i32(out, 2);
            write_len_bytes(out, &encode_big_integer(*value));
        }
        _ => panic!("HSQLDB cell/type mismatch: {cell:?} for {ty:?}"),
    }
}

fn read_u8(buf: &[u8], pos: usize) -> Result<(u8, usize), HsqlError> {
    let bytes = read_exact(buf, pos, 1)?;
    Ok((bytes[0], pos + 1))
}

fn read_i32(buf: &[u8], pos: usize) -> Result<(i32, usize), HsqlError> {
    let bytes = read_exact(buf, pos, 4)?;
    let mut array = [0_u8; 4];
    array.copy_from_slice(bytes);
    Ok((i32::from_be_bytes(array), pos + 4))
}

fn read_i64(buf: &[u8], pos: usize) -> Result<(i64, usize), HsqlError> {
    let bytes = read_exact(buf, pos, 8)?;
    let mut array = [0_u8; 8];
    array.copy_from_slice(bytes);
    Ok((i64::from_be_bytes(array), pos + 8))
}

fn read_len_bytes(buf: &[u8], pos: usize) -> Result<(&[u8], usize), HsqlError> {
    let (len, pos) = read_i32(buf, pos)?;
    let len = usize::try_from(len).map_err(|_| HsqlError::negative_length(len))?;
    let bytes = read_exact(buf, pos, len)?;
    Ok((bytes, pos + len))
}

fn read_exact(buf: &[u8], pos: usize, len: usize) -> Result<&[u8], HsqlError> {
    let end = pos
        .checked_add(len)
        .ok_or_else(|| HsqlError::unexpected_eof(pos, len, buf.len()))?;
    if end > buf.len() {
        return Err(HsqlError::unexpected_eof(pos, len, buf.len()));
    }
    Ok(&buf[pos..end])
}

fn write_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn write_i64(out: &mut Vec<u8>, value: i64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn write_len_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    let len = i32::try_from(bytes.len()).expect("HSQLDB byte length fits in i32");
    write_i32(out, len);
    out.extend_from_slice(bytes);
}

fn date_from_millis(millis: i64) -> Result<String, HsqlError> {
    let seconds = millis.div_euclid(MILLIS_PER_SECOND);
    let date = OffsetDateTime::from_unix_timestamp(seconds)
        .map_err(|_| HsqlError::invalid_date_millis(millis))?
        .date();
    Ok(format_date(date))
}

fn millis_from_date(value: &str) -> i64 {
    let date = parse_date(value).expect("HSQLDB DATE cell uses YYYY-MM-DD");
    date.with_hms(0, 0, 0)
        .expect("midnight is valid")
        .assume_utc()
        .unix_timestamp()
        .checked_mul(MILLIS_PER_SECOND)
        .expect("HSQLDB DATE millis fit in i64")
}

fn parse_date(value: &str) -> Option<Date> {
    let mut parts = value.split('-');
    let year = parts.next()?.parse().ok()?;
    let month: u8 = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    let month = Month::try_from(month).ok()?;
    Date::from_calendar_date(year, month, day).ok()
}

fn format_date(date: Date) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}

fn decode_big_integer(bytes: &[u8]) -> Result<i64, HsqlError> {
    if bytes.is_empty() {
        return Err(HsqlError::invalid_big_integer());
    }
    let negative = bytes[0] & 0x80 != 0;
    let fill = if negative { 0xff } else { 0x00 };
    let mut start = 0;
    while bytes.len() - start > 8 {
        let next_sign_matches = (bytes[start + 1] & 0x80 != 0) == negative;
        if bytes[start] != fill || !next_sign_matches {
            return Err(HsqlError::numeric_overflow());
        }
        start += 1;
    }

    let mut out = [fill; 8];
    let payload = &bytes[start..];
    out[8 - payload.len()..].copy_from_slice(payload);
    Ok(i64::from_be_bytes(out))
}

fn encode_big_integer(value: i64) -> Vec<u8> {
    let mut bytes = value.to_be_bytes().to_vec();
    while bytes.len() > 1 {
        let redundant_positive = bytes[0] == 0 && bytes[1] & 0x80 == 0;
        let redundant_negative = bytes[0] == 0xff && bytes[1] & 0x80 != 0;
        if redundant_positive || redundant_negative {
            bytes.remove(0);
        } else {
            break;
        }
    }
    bytes
}

fn to_minor_units(unscaled: i64, scale: i32) -> Result<i64, HsqlError> {
    if scale > 2 {
        return Err(HsqlError::unsupported_numeric_scale(scale));
    }
    let exponent = 2_i32 - scale;
    let factor = pow10(exponent)?;
    unscaled
        .checked_mul(factor)
        .ok_or_else(HsqlError::numeric_overflow)
}

fn pow10(exponent: i32) -> Result<i64, HsqlError> {
    let exponent = u32::try_from(exponent).map_err(|_| HsqlError::numeric_overflow())?;
    let mut value = 1_i64;
    for _ in 0..exponent {
        value = value
            .checked_mul(10)
            .ok_or_else(HsqlError::numeric_overflow)?;
    }
    Ok(value)
}

fn checked_i32(value: i64) -> i32 {
    i32::try_from(value).expect("HSQLDB INTEGER cell fits in i32")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_all_supported_column_types() {
        let cases = [
            (Cell::Null, ColType::Integer),
            (Cell::Null, ColType::Varchar),
            (Cell::Null, ColType::Date),
            (Cell::Null, ColType::Numeric),
            (Cell::Int(-42), ColType::Integer),
            (Cell::Str("konto-rad alpha".to_owned()), ColType::Varchar),
            (
                Cell::Str("r\u{00e4}nta p\u{00e5} \u{00e5}ret".to_owned()),
                ColType::Varchar,
            ),
            (Cell::Date("2024-02-29".to_owned()), ColType::Date),
            (Cell::Num(123_456), ColType::Numeric),
            (Cell::Num(-123_456), ColType::Numeric),
        ];

        for (cell, ty) in cases {
            let mut bytes = Vec::new();
            write_cell(&mut bytes, &cell, ty);
            let (decoded, pos) = read_cell(&bytes, 0, ty).unwrap();
            assert_eq!(decoded, cell);
            assert_eq!(pos, bytes.len());
        }
    }

    #[test]
    fn reads_numeric_scales_as_minor_units() {
        assert_eq!(read_numeric(0, 1_234).unwrap(), Cell::Num(123_400));
        assert_eq!(read_numeric(1, 1_234).unwrap(), Cell::Num(12_340));
        assert_eq!(read_numeric(2, 1_234).unwrap(), Cell::Num(1_234));
    }

    #[test]
    fn rejects_unsupported_numeric_scale() {
        let err = read_numeric(3, 1_234).unwrap_err();
        assert!(err.to_string().contains("unsupported HSQLDB NUMERIC scale"));
    }

    #[test]
    fn fails_closed_on_truncated_input() {
        let err = read_cell(&[PRESENT_MARKER, 0, 0], 0, ColType::Integer).unwrap_err();
        assert!(err.to_string().contains("unexpected end"));
    }

    fn read_numeric(scale: i32, unscaled: i64) -> Result<Cell, HsqlError> {
        let mut bytes = vec![PRESENT_MARKER];
        write_i32(&mut bytes, scale);
        write_len_bytes(&mut bytes, &encode_big_integer(unscaled));
        let (cell, pos) = read_cell(&bytes, 0, ColType::Numeric)?;
        assert_eq!(pos, bytes.len());
        Ok(cell)
    }
}
