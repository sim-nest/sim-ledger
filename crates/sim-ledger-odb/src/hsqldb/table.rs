//! HSQLDB cached-table row traversal.

use std::collections::BTreeSet;

use crate::ColType;
use crate::hsqldb::row::{Cell, HsqlError, read_cell};

/// Read every row reachable from a table's primary index root.
pub fn read_table(
    data: &[u8],
    table_root: i64,
    cols: &[(String, ColType)],
    index_count: usize,
) -> Result<Vec<Vec<Cell>>, HsqlError> {
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    walk_row(data, table_root, cols, index_count, &mut seen, &mut rows)?;
    Ok(rows)
}

fn walk_row(
    data: &[u8],
    offset: i64,
    cols: &[(String, ColType)],
    index_count: usize,
    seen: &mut BTreeSet<i64>,
    rows: &mut Vec<Vec<Cell>>,
) -> Result<(), HsqlError> {
    if offset <= 0 {
        return Ok(());
    }
    if !seen.insert(offset) {
        return Err(HsqlError::row_cycle(offset));
    }

    let row = decode_row(data, offset, cols, index_count)?;
    walk_row(data, row.left, cols, index_count, seen, rows)?;
    rows.push(row.cells);
    walk_row(data, row.right, cols, index_count, seen, rows)
}

fn decode_row(
    data: &[u8],
    offset: i64,
    cols: &[(String, ColType)],
    index_count: usize,
) -> Result<RowBlock, HsqlError> {
    let offset = usize::try_from(offset).map_err(|_| HsqlError::invalid_row_offset(offset))?;
    let (size, mut pos) = read_i32(data, offset)?;
    let size = usize::try_from(size).map_err(|_| HsqlError::invalid_row_size(size))?;
    if size < 4 {
        return Err(HsqlError::invalid_row_size(
            i32::try_from(size).unwrap_or(i32::MAX),
        ));
    }
    let end = offset
        .checked_add(size)
        .ok_or_else(|| HsqlError::unexpected_eof(offset, size, data.len()))?;
    if end > data.len() {
        return Err(HsqlError::unexpected_eof(offset, size, data.len()));
    }

    let mut left = 0_i64;
    let mut right = 0_i64;
    for index in 0..index_count {
        let (_, next) = read_i32(data, pos)?;
        let (node_left, next) = read_i32(data, next)?;
        let (node_right, next) = read_i32(data, next)?;
        let (_, next) = read_i32(data, next)?;
        if index == 0 {
            left = i64::from(node_left);
            right = i64::from(node_right);
        }
        pos = next;
        if pos > end {
            return Err(HsqlError::unexpected_eof(end, pos - end, end));
        }
    }

    let mut cells = Vec::with_capacity(cols.len());
    for (_, ty) in cols {
        let (cell, next) = read_cell(data, pos, *ty)?;
        if next > end {
            return Err(HsqlError::unexpected_eof(pos, next - pos, end));
        }
        cells.push(cell);
        pos = next;
    }

    Ok(RowBlock { left, right, cells })
}

struct RowBlock {
    left: i64,
    right: i64,
    cells: Vec<Cell>,
}

fn read_i32(buf: &[u8], pos: usize) -> Result<(i32, usize), HsqlError> {
    let end = pos
        .checked_add(4)
        .ok_or_else(|| HsqlError::unexpected_eof(pos, 4, buf.len()))?;
    if end > buf.len() {
        return Err(HsqlError::unexpected_eof(pos, 4, buf.len()));
    }
    let mut array = [0_u8; 4];
    array.copy_from_slice(&buf[pos..end]);
    Ok((i32::from_be_bytes(array), end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ColType;
    use crate::hsqldb::try_write_cell;

    #[test]
    fn walks_primary_index_in_order() {
        let cols = vec![
            ("id".to_owned(), ColType::Integer),
            ("name".to_owned(), ColType::Varchar),
        ];
        let mut data = vec![0; 16];
        let left = append_row(
            &mut data,
            0,
            0,
            0,
            &[
                (Cell::Int(1), ColType::Integer),
                (Cell::Str("a".to_owned()), ColType::Varchar),
            ],
        );
        let root = append_row(
            &mut data,
            left,
            0,
            0,
            &[
                (Cell::Int(2), ColType::Integer),
                (Cell::Str("b".to_owned()), ColType::Varchar),
            ],
        );

        let rows = read_table(&data, i64::from(root), &cols, 2).unwrap();
        assert_eq!(
            rows,
            vec![
                vec![Cell::Int(1), Cell::Str("a".to_owned())],
                vec![Cell::Int(2), Cell::Str("b".to_owned())],
            ]
        );
    }

    pub(crate) fn append_row(
        data: &mut Vec<u8>,
        left: i32,
        right: i32,
        parent: i32,
        cells: &[(Cell, ColType)],
    ) -> i32 {
        let offset = i32::try_from(data.len()).unwrap();
        let mut body = Vec::new();
        body.extend_from_slice(&0_i32.to_be_bytes());
        body.extend_from_slice(&left.to_be_bytes());
        body.extend_from_slice(&right.to_be_bytes());
        body.extend_from_slice(&parent.to_be_bytes());
        body.extend_from_slice(&0_i32.to_be_bytes());
        body.extend_from_slice(&0_i32.to_be_bytes());
        body.extend_from_slice(&0_i32.to_be_bytes());
        body.extend_from_slice(&0_i32.to_be_bytes());
        for (cell, ty) in cells {
            try_write_cell(&mut body, cell, *ty).unwrap();
        }
        let row_size = i32::try_from(body.len() + 4).unwrap();
        data.extend_from_slice(&row_size.to_be_bytes());
        data.extend_from_slice(&body);
        offset
    }
}
