//! Directory entry wrapper for ergonomic access.

use crate::{ArchivePath, CentralDirectoryHeader, DosDateTime};

/// A directory entry in the archive.
///
/// This struct wraps a Central Directory header for an explicit directory entry.
/// Note that ZIP archives may have implicit directories (directories that exist
/// only because files have paths containing them). This struct only represents
/// explicit directory entries that were stored in the archive.
///
/// # Lifetime
///
/// The lifetime `'a` is tied to the archive that created this entry.
#[derive(Debug)]
pub struct DirEntry<'a> {
    /// The Central Directory header for this directory.
    pub(crate) header: &'a CentralDirectoryHeader,
    /// The directory path (without null padding or trailing slash).
    pub(crate) path: ArchivePath<'a>,
}

impl<'a> DirEntry<'a> {
    /// Returns the directory path.
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
