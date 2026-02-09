//! File entry wrapper for ergonomic access.

use crate::ArchivePath;
use crate::format::EntryRow;

/// A file entry in the archive.
///
/// This struct wraps an entry row and provides convenient access to file
/// metadata and data. The data is pre-resolved for ergonomics.
///
/// # Lifetime
///
/// The lifetime `'a` is tied to the archive that created this entry.
/// All borrowed data (path, entry row, file data) remains valid for this lifetime.
#[derive(Debug)]
pub struct FileEntry<'a> {
    /// The entry row for this file.
    pub(crate) entry: &'a EntryRow,
    /// The file path (without null padding).
    pub(crate) path: ArchivePath<'a>,
    /// The file data.
    pub(crate) data: &'a [u8],
    /// Stable entry ID.
    pub(crate) id: u32,
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
        self.entry.file_size.get()
    }

    /// Returns the file path.
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
