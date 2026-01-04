//! Symlink entry wrapper for ergonomic access.

use crate::{ArchivePath, CentralDirectoryHeader, DosDateTime};

/// A symbolic link entry in the archive.
///
/// This struct wraps a Central Directory header for a symlink entry and
/// provides convenient access to the link metadata and target.
///
/// # Lifetime
///
/// The lifetime `'a` is tied to the archive that created this entry.
/// All borrowed data (path, header, target) remains valid for this lifetime.
#[derive(Debug)]
pub struct SymlinkEntry<'a> {
    /// The Central Directory header for this symlink.
    pub(crate) header: &'a CentralDirectoryHeader,
    /// The symlink path (without null padding).
    pub(crate) path: ArchivePath<'a>,
    /// The symlink target (stored as file data).
    pub(crate) target: &'a [u8],
}

impl<'a> SymlinkEntry<'a> {
    /// Returns the symlink target as bytes.
    ///
    /// The target is stored as the file data of the symlink entry.
    #[must_use]
    pub fn target_bytes(&self) -> &'a [u8] {
        self.target
    }

    /// Returns the symlink target as a string, if valid UTF-8.
    ///
    /// Returns `None` if the target is not valid UTF-8.
    #[must_use]
    pub fn target(&self) -> Option<&'a str> {
        std::str::from_utf8(self.target).ok()
    }

    /// Returns the symlink path.
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
