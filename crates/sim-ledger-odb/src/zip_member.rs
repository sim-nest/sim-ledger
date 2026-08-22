//! ZIP member access over caller-supplied `.odb` content.
use std::io::{self, Cursor, Read};
use zip::{ZipArchive, result::ZipError};
/// Read one member from supplied ODB bytes.
pub fn open_zip_member(odb: &[u8], member: &str) -> io::Result<Vec<u8>> {
    let mut archive = ZipArchive::new(Cursor::new(odb)).map_err(zip_error)?;
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
