//! Read operations trait for archives.

use crate::archive::{DirEntry, Entry, FileEntry, SymlinkEntry};
use crate::format::{EntryRow, Trailer};
use crate::{ArchivePath, BaleError};

/// Read operations for archives.
///
/// This trait is implemented for all `Archive<M>` where `M` provides byte access.
pub trait ArchiveRead {
    /// Returns the number of entries in the archive.
    fn entry_count(&self) -> usize;

    /// Returns the configured path size for this archive.
    fn path_size(&self) -> usize;

    /// Returns the configured alignment for this archive.
    ///
    /// # Panics
    ///
    /// Panics if `alignment_power` is invalid. This cannot happen for archives
    /// opened via [`open()`](super::Archive::open) since validation occurs on construction.
    fn alignment(&self) -> u32;

    /// Returns the path for the entry at the given index as a zero-copy `ArchivePath`.
    ///
    /// The returned path borrows directly from the mmap.
    /// Returns `None` if the index is out of bounds.
    fn get_path(&self, index: usize) -> Option<ArchivePath<'_>>;

    /// Returns an iterator over all entry rows.
    ///
    /// Each item is a tuple of (entry_row, path_bytes) where path_bytes is the
    /// null-padded path from the directory table.
    fn iter_entries(&self) -> impl Iterator<Item = (&EntryRow, &[u8])>;

    /// Finds an entry row by path.
    ///
    /// Returns the entry row for the given path, or `None` if not found.
    /// The directory table is sorted by path, enabling binary search.
    fn find_entry(&self, path: &str) -> Option<&EntryRow>;

    /// Finds an entry by path and returns entry row, trimmed path bytes, and ID.
    ///
    /// Like [`find_entry`](Self::find_entry), but also returns the path bytes
    /// from the archive (with null padding removed) and the stable entry ID.
    fn find_entry_with_path(&self, path: &str) -> Option<(&EntryRow, &[u8], u32)>;

    /// Returns a zero-copy slice of the file data for the given entry row.
    ///
    /// # Errors
    ///
    /// Returns an error if the entry's data offset or size is invalid.
    fn read_data(&self, entry: &EntryRow) -> Result<&[u8], BaleError>;

    /// Returns a reference to the archive trailer.
    fn trailer(&self) -> &Trailer;

    /// Verifies the CRC-32 checksum for an entry.
    ///
    /// Reads the entry data and computes its CRC-32, comparing against the
    /// stored value in the data block header.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The entry data cannot be read
    /// - The computed CRC does not match the stored CRC
    fn verify_crc(&self, entry: &EntryRow) -> Result<(), BaleError>;

    /// Checks if the directory table is sorted by path bytes.
    ///
    /// A sorted directory table enables binary search for entry lookup.
    /// Archives created by the writer are always sorted.
    fn is_sorted(&self) -> bool;

    /// Returns a list of duplicate paths in the archive.
    ///
    /// Duplicate paths occur when the same path appears multiple times in the
    /// directory table (e.g., hard links with the same path are not duplicates
    /// since they share the same entry ID).
    fn find_duplicates(&self) -> Vec<ArchivePath<'static>>;

    /// Checks if the archive contains orphaned data.
    ///
    /// Orphaned data exists when there are data blocks not referenced by any
    /// entry in the entry table.
    fn has_orphaned_data(&self) -> bool;

    /// Returns a file entry by path.
    ///
    /// This method provides type-safe access to file entries without manual
    /// kind checking. The path is normalized before lookup.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The path is not found ([`BaleError::EntryNotFound`])
    /// - The entry exists but is not a file ([`BaleError::NotAFile`])
    /// - The entry data cannot be read
    fn file(&self, path: impl AsRef<str>) -> Result<FileEntry<'_>, BaleError>;

    /// Returns a directory entry by path.
    ///
    /// This method provides type-safe access to directory entries without manual
    /// kind checking. The path is normalized before lookup (trailing slashes
    /// are handled automatically).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The path is not found ([`BaleError::EntryNotFound`])
    /// - The entry exists but is not a directory ([`BaleError::NotADirectory`])
    fn folder(&self, path: impl AsRef<str>) -> Result<DirEntry<'_>, BaleError>;

    /// Returns a symlink entry by path.
    ///
    /// This method provides type-safe access to symlink entries without manual
    /// kind checking. The path is normalized before lookup.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The path is not found ([`BaleError::EntryNotFound`])
    /// - The entry exists but is not a symlink ([`BaleError::NotASymlink`])
    /// - The entry data cannot be read
    fn symlink(&self, path: impl AsRef<str>) -> Result<SymlinkEntry<'_>, BaleError>;

    /// Returns any entry by path.
    ///
    /// This method returns a generic [`Entry`] enum that can be matched on to
    /// determine the entry type. Use this when you need to handle any type of
    /// entry, or when you don't know the type ahead of time.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The path is not found ([`BaleError::EntryNotFound`])
    /// - The entry data cannot be read (for files and symlinks)
    fn entry(&self, path: impl AsRef<str>) -> Result<Entry<'_>, BaleError>;

    /// Finds an entry by its stable ID.
    ///
    /// Returns `None` if no entry with the given ID exists.
    fn find_by_id(&self, id: u32) -> Option<Entry<'_>>;
}
