//! Symlink entry wrapper for ergonomic access.

use crate::ArchivePath;
use crate::format::EntryRow;

/// A symbolic link entry in the archive.
///
/// This struct wraps an entry row for a symlink entry and provides convenient
/// access to the link metadata and target. The target is stored as the data
/// block content (raw UTF-8 bytes without a null terminator).
///
/// # Lifetime
///
/// The lifetime `'a` is tied to the archive that created this entry.
/// All borrowed data (path, entry row, target) remains valid for this lifetime.
#[derive(Debug)]
pub struct SymlinkEntry<'a> {
    /// The entry row for this symlink.
    pub(crate) entry: &'a EntryRow,
    /// The symlink path (without null padding).
    pub(crate) path: ArchivePath<'a>,
    /// The symlink target (stored as data block content).
    pub(crate) target: &'a [u8],
    /// Stable entry ID.
    pub(crate) id: u32,
}

impl<'a> SymlinkEntry<'a> {
    /// Returns the symlink target as bytes.
    ///
    /// The target is stored as the data block content of the symlink entry.
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
    #[must_use]
    pub fn mode(&self) -> u32 {
        self.entry.mode.get()
    }

    /// Returns the creation time as Unix epoch milliseconds.
    #[must_use]
    pub fn created_time(&self) -> i64 {
        self.entry.created_time.get()
    }

    /// Returns the modification time as Unix epoch milliseconds.
    #[must_use]
    pub fn modified_time(&self) -> i64 {
        self.entry.modified_time.get()
    }

    /// Returns a reference to the entry row.
    #[must_use]
    pub fn entry(&self) -> &'a EntryRow {
        self.entry
    }

    /// Returns the stable entry ID.
    #[must_use]
    pub fn id(&self) -> u32 {
        self.id
    }
}
