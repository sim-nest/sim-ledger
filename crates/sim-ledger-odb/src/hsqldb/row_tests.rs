use crate::ColType;
use crate::hsqldb::row::checked_hsqldb_len;
use crate::hsqldb::{Cell, HsqlError, WriteCellError, read_cell, try_write_cell};

const PRESENT_MARKER: u8 = 1;

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
        try_write_cell(&mut bytes, &cell, ty).unwrap();
        let (decoded, pos) = read_cell(&bytes, 0, ty).unwrap();
        assert_eq!(decoded, cell);
        assert_eq!(pos, bytes.len());
    }
}

#[test]
fn rejects_cell_type_mismatches_without_writing() {
    let mut bytes = Vec::new();
    let err = try_write_cell(
        &mut bytes,
        &Cell::Str("not int".to_owned()),
        ColType::Integer,
    )
    .unwrap_err();

    assert_eq!(
        err,
        WriteCellError::TypeMismatch {
            cell: "str",
            ty: ColType::Integer
        }
    );
    assert!(bytes.is_empty());
}

#[test]
fn rejects_out_of_range_integer_without_writing() {
    let mut bytes = Vec::new();
    let err = try_write_cell(
        &mut bytes,
        &Cell::Int(i64::from(i32::MAX) + 1),
        ColType::Integer,
    )
    .unwrap_err();

    assert_eq!(
        err,
        WriteCellError::IntegerOutOfRange {
            value: i64::from(i32::MAX) + 1
        }
    );
    assert!(bytes.is_empty());
}

#[test]
fn rejects_invalid_dates_without_writing() {
    let mut bytes = Vec::new();
    let err = try_write_cell(
        &mut bytes,
        &Cell::Date("2024-02-31".to_owned()),
        ColType::Date,
    )
    .unwrap_err();

    assert_eq!(
        err,
        WriteCellError::InvalidDate {
            value: "2024-02-31".to_owned()
        }
    );
    assert!(bytes.is_empty());
}

#[test]
fn rejects_length_prefix_overflow() {
    let len = i32::MAX as usize + 1;
    assert_eq!(
        checked_hsqldb_len(len).unwrap_err(),
        WriteCellError::LengthOutOfRange { len }
    );
}

#[test]
fn reads_numeric_scales_as_minor_units() {
    assert_eq!(read_numeric(0, 1_234).unwrap(), Cell::Num(123_400));
    assert_eq!(read_numeric(1, 1_234).unwrap(), Cell::Num(12_340));
    assert_eq!(read_numeric(2, 1_234).unwrap(), Cell::Num(1_234));
}

#[test]
fn reads_hsqldb_numeric_payload_before_scale() {
    let bytes = [PRESENT_MARKER, 0, 0, 0, 2, 0x04, 0xd2, 0, 0, 0, 2];
    let (cell, pos) = read_cell(&bytes, 0, ColType::Numeric).unwrap();
    assert_eq!(cell, Cell::Num(1_234));
    assert_eq!(pos, bytes.len());
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
    write_len_bytes(&mut bytes, &encode_big_integer(unscaled));
    write_i32(&mut bytes, scale);
    let (cell, pos) = read_cell(&bytes, 0, ColType::Numeric)?;
    assert_eq!(pos, bytes.len());
    Ok(cell)
}

fn write_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn write_len_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    let len = i32::try_from(bytes.len()).unwrap();
    write_i32(out, len);
    out.extend_from_slice(bytes);
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
