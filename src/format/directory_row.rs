//! Directory table row mapping paths to entry IDs.

use crate::BaleError;

/// A directory table row mapping a path to an entry ID.
///
/// Unlike the other format structs, `DirectoryRow` is NOT a zerocopy struct
/// because the row stride depends on the runtime `path_size` configuration.
/// Instead, it wraps a borrowed byte slice and provides accessor methods.
///
/// # Layout
///
/// | Offset      | Size        | Field    | Description                    |
/// |-------------|-------------|----------|--------------------------------|
/// | 0           | `path_size` | Path     | UTF-8, null-padded             |
/// | `path_size` | 4           | Entry ID | LE, references an entry row    |
///
/// **Stride:** `path_size + 4` bytes per row.
#[derive(Debug, Clone, Copy)]
pub struct DirectoryRow<'a> {
    /// The raw row bytes (path_size + 4 bytes).
    data: &'a [u8],
    /// The path size for this archive (determines stride).
    path_size: u16,
}

impl<'a> DirectoryRow<'a> {
    /// Size of the entry ID field in bytes.
    pub const ENTRY_ID_SIZE: usize = 4;

    /// Returns the stride (total row size) for a given path size.
    #[must_use]
    pub const fn stride(path_size: u16) -> usize {
        path_size as usize + Self::ENTRY_ID_SIZE
    }

    /// Creates a new `DirectoryRow` from a byte slice.
    ///
    /// The slice must be exactly `path_size + 4` bytes.
    ///
    /// # Errors
    ///
    /// Returns `BaleError::Corrupted` if the slice length does not match
    /// the expected stride.
    pub fn from_bytes(data: &'a [u8], path_size: u16) -> Result<Self, BaleError> {
        let expected = Self::stride(path_size);
        if data.len() != expected {
            return Err(BaleError::Corrupted(format!(
                "directory row size mismatch: expected {expected}, got {}",
                data.len()
            )));
        }
        Ok(Self { data, path_size })
    }

    /// Returns the raw path bytes (including null padding).
    #[must_use]
    pub fn path_bytes_raw(&self) -> &'a [u8] {
        &self.data[..self.path_size as usize]
    }

    /// Returns the path bytes with null padding removed.
    #[must_use]
    pub fn path_bytes(&self) -> &'a [u8] {
        let raw = self.path_bytes_raw();
        let len = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        &raw[..len]
    }

    /// Returns the path as a UTF-8 string (null padding removed).
    ///
    /// # Errors
    ///
    /// Returns `BaleError::InvalidUtf8` if the path is not valid UTF-8.
    pub fn path(&self) -> Result<&'a str, BaleError> {
        Ok(std::str::from_utf8(self.path_bytes())?)
    }

    /// Returns the entry ID (little-endian u32).
    #[must_use]
    pub fn entry_id(&self) -> u32 {
        let offset = self.path_size as usize;
        let bytes = &self.data[offset..offset + Self::ENTRY_ID_SIZE];
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stride is path_size + 4.
    #[test]
    fn stride() {
        assert_eq!(DirectoryRow::stride(256), 260);
        assert_eq!(DirectoryRow::stride(1), 5);
        assert_eq!(DirectoryRow::stride(4096), 4100);
    }

    /// Valid row is parsed correctly.
    #[test]
    fn valid_row() {
        let path_size: u16 = 16;
        let mut data = vec![0u8; DirectoryRow::stride(path_size)];
        // Write "hello.txt" as path
        data[..9].copy_from_slice(b"hello.txt");
        // Write entry ID = 42 (LE)
        let id_offset = path_size as usize;
        data[id_offset..id_offset + 4].copy_from_slice(&42u32.to_le_bytes());

        let row = DirectoryRow::from_bytes(&data, path_size).unwrap();
        assert_eq!(row.path_bytes(), b"hello.txt");
        assert_eq!(row.path().unwrap(), "hello.txt");
        assert_eq!(row.entry_id(), 42);
    }

    /// Full-length path (no null padding) is handled.
    #[test]
    fn full_length_path() {
        let path_size: u16 = 5;
        let mut data = vec![0u8; DirectoryRow::stride(path_size)];
        data[..5].copy_from_slice(b"abcde");
        data[5..9].copy_from_slice(&1u32.to_le_bytes());

        let row = DirectoryRow::from_bytes(&data, path_size).unwrap();
        assert_eq!(row.path_bytes(), b"abcde");
        assert_eq!(row.entry_id(), 1);
    }

    /// Wrong-sized slice returns error.
    #[test]
    fn wrong_size_returns_error() {
        let data = vec![0u8; 10];
        let result = DirectoryRow::from_bytes(&data, 256);
        assert!(result.is_err());
    }

    /// Raw path bytes include null padding.
    #[test]
    fn raw_path_includes_padding() {
        let path_size: u16 = 16;
        let mut data = vec![0u8; DirectoryRow::stride(path_size)];
        data[..3].copy_from_slice(b"src");

        let row = DirectoryRow::from_bytes(&data, path_size).unwrap();
        assert_eq!(row.path_bytes_raw().len(), 16);
        assert_eq!(row.path_bytes(), b"src");
    }

    /// Invalid UTF-8 in path returns error.
    #[test]
    fn invalid_utf8_returns_error() {
        let path_size: u16 = 8;
        let mut data = vec![0u8; DirectoryRow::stride(path_size)];
        data[0] = 0xFF;
        data[1] = 0xFE;

        let row = DirectoryRow::from_bytes(&data, path_size).unwrap();
        assert!(row.path().is_err());
    }
}
