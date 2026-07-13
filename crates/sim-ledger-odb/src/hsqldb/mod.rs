//! HSQLDB 1.8 binary row helpers.

pub mod row;

pub use row::{Cell, HsqlError, read_cell, write_cell};
