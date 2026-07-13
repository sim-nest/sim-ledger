//! ZIP member access for `.odb` files.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use zip::ZipArchive;
use zip::result::ZipError;

/// Read one member from an `.odb` ZIP container.
pub fn open_zip_member(odb: &Path, member: &str) -> io::Result<Vec<u8>> {
    let file = File::open(odb)?;
    let mut archive = ZipArchive::new(file).map_err(zip_error)?;
    let mut entry = archive.by_name(member).map_err(zip_error)?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn zip_error(err: ZipError) -> io::Error {
    let kind = match err {
        ZipError::FileNotFound => io::ErrorKind::NotFound,
        _ => io::ErrorKind::InvalidData,
    };
    io::Error::new(kind, err)
}
