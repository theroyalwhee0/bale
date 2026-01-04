//! File entry wrapper for ergonomic access.

use crate::{ArchivePath, CentralDirectoryHeader, DosDateTime};

/// A file entry in the archive.
///
/// This struct wraps a Central Directory header and provides convenient
/// access to file metadata and data. The data is pre-resolved for ergonomics.
///
/// # Lifetime
///
/// The lifetime `'a` is tied to the archive that created this entry.
/// All borrowed data (path, header, file data) remains valid for this lifetime.
#[derive(Debug)]
pub struct FileEntry<'a> {
    /// The Central Directory header for this file.
    pub(crate) header: &'a CentralDirectoryHeader,
    /// The file path (without null padding).
    pub(crate) path: ArchivePath<'a>,
    /// The file data.
    pub(crate) data: &'a [u8],
}

impl<'a> FileEntry<'a> {
    /// Returns the file data.
    #[must_use]
    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    /// Returns the uncompressed file size in bytes.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.header.uncompressed_size.get() as u64
    }

    /// Returns the CRC-32 checksum of the file data.
    #[must_use]
    pub fn crc32(&self) -> u32 {
        self.header.crc32.get()
    }

    /// Returns the file path.
    #[must_use]
    pub fn path(&self) -> &ArchivePath<'a> {
        &self.path
    }

    /// Returns the Unix mode (file type and permissions).
    ///
    /// The mode is extracted from the upper 16 bits of `external_attrs`.
    #[must_use]
    pub fn mode(&self) -> u32 {
        self.header.external_attrs.get() >> 16
    }

    /// Returns the modification time.
    #[must_use]
    pub fn mtime(&self) -> DosDateTime {
        DosDateTime::from_date_time_parts(self.header.mod_date.get(), self.header.mod_time.get())
    }

    /// Returns a reference to the Central Directory header.
    #[must_use]
    pub fn header(&self) -> &'a CentralDirectoryHeader {
        self.header
    }
}
