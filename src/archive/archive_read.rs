//! Read operations trait for archives.

use crate::{ArchivePath, BaleEocd, BaleError, CentralDirectoryHeader};

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
    /// Panics if `alignment_pow2` is invalid. This cannot happen for archives
    /// opened via [`open()`](super::Archive::open) since validation occurs on construction.
    fn alignment(&self) -> u32;

    /// Returns the path for the entry at the given index as a zero-copy `ArchivePath`.
    ///
    /// The returned path borrows directly from the in-memory entries.
    /// Returns `None` if the index is out of bounds.
    fn get_path(&self, index: usize) -> Option<ArchivePath<'_>>;

    /// Returns an iterator over all Central Directory entries.
    ///
    /// Each item is a tuple of (header, path_bytes) where path_bytes is the
    /// null-padded path from the CD entry.
    fn iter_entries(&self) -> impl Iterator<Item = (&CentralDirectoryHeader, &[u8])>;

    /// Finds an entry by path using linear scan.
    ///
    /// Returns the last matching entry. Bale uses append-only shadowing: when
    /// a file is updated, the new version is appended and the old version
    /// remains but is "shadowed". The last occurrence is the current version.
    ///
    /// The path comparison is byte-exact against the null-padded path.
    fn find_entry(&self, path: &str) -> Option<&CentralDirectoryHeader>;

    /// Returns a zero-copy slice of the file data for the given entry.
    ///
    /// # Errors
    ///
    /// Returns an error if the entry's offset or size is invalid.
    fn read_data(&self, entry: &CentralDirectoryHeader) -> Result<&[u8], BaleError>;

    /// Returns a reference to the BaleEocd.
    fn bale_eocd(&self) -> &BaleEocd;

    /// Verifies the CRC-32 checksum for an entry.
    ///
    /// Reads the entry data and computes its CRC-32, comparing against the
    /// stored value in the Central Directory header.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The entry data cannot be read
    /// - The computed CRC does not match the stored CRC
    fn verify_crc(&self, entry: &CentralDirectoryHeader) -> Result<(), BaleError>;

    /// Checks if the Central Directory is sorted by path bytes.
    ///
    /// A sorted CD enables binary search for entry lookup. Archives created
    /// by `compact` are always sorted.
    fn is_sorted(&self) -> bool;

    /// Returns a list of duplicate paths in the archive.
    ///
    /// Duplicate paths occur when the same path appears multiple times in the
    /// Central Directory (shadowing). Returns the paths that have duplicates,
    /// not the total count of duplicates.
    fn find_duplicates(&self) -> Vec<String>;

    /// Checks if the archive contains orphaned data.
    ///
    /// Orphaned data exists when there are gaps between entries or between
    /// the last entry and the Central Directory. This can occur after
    /// deletions or when entries are shadowed.
    fn has_orphaned_data(&self) -> bool;
}
