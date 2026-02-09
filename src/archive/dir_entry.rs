//! Directory entry wrapper for ergonomic access.

use crate::ArchivePath;
use crate::format::EntryRow;

/// A directory entry in the archive.
///
/// This struct wraps an entry row for a directory. In the bale format,
/// directories are explicit entries in both the entry table and directory table.
///
/// # Lifetime
///
/// The lifetime `'a` is tied to the archive that created this entry.
#[derive(Debug)]
pub struct DirEntry<'a> {
    /// The entry row for this directory.
    pub(crate) entry: &'a EntryRow,
    /// The directory path (without null padding or trailing slash).
    pub(crate) path: ArchivePath<'a>,
    /// Stable entry ID.
    pub(crate) id: u32,
}

impl<'a> DirEntry<'a> {
    /// Returns the directory path.
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
