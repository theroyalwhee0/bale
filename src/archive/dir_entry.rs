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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ArchivePath;

    /// Creates a test directory `EntryRow` with known values.
    fn test_entry_row() -> EntryRow {
        EntryRow::new_directory(7, 1_700_000_000_000, 1_700_000_001_000, 0o040755)
    }

    /// Creates a `DirEntry` referencing the given row.
    fn make_dir_entry(row: &EntryRow) -> DirEntry<'_> {
        DirEntry {
            entry: row,
            path: ArchivePath::from_bytes(b"src/lib"),
            id: 7,
        }
    }

    /// `path()` returns the directory path.
    #[test]
    fn path_returns_archive_path() {
        let row = test_entry_row();
        let dir = make_dir_entry(&row);
        assert_eq!(dir.path().as_bytes(), b"src/lib");
    }

    /// `mode()` returns the Unix mode from the entry row.
    #[test]
    fn mode_returns_unix_mode() {
        let row = test_entry_row();
        let dir = make_dir_entry(&row);
        assert_eq!(dir.mode(), 0o040755);
    }

    /// `created_time()` returns the creation timestamp.
    #[test]
    fn created_time_returns_timestamp() {
        let row = test_entry_row();
        let dir = make_dir_entry(&row);
        assert_eq!(dir.created_time(), 1_700_000_000_000);
    }

    /// `modified_time()` returns the modification timestamp.
    #[test]
    fn modified_time_returns_timestamp() {
        let row = test_entry_row();
        let dir = make_dir_entry(&row);
        assert_eq!(dir.modified_time(), 1_700_000_001_000);
    }

    /// `entry()` returns a reference to the underlying entry row.
    #[test]
    fn entry_returns_row_reference() {
        let row = test_entry_row();
        let dir = make_dir_entry(&row);
        let returned = dir.entry();
        assert_eq!(returned.entry_id.get(), 7);
        assert_eq!(returned.mode.get(), 0o040755);
    }

    /// `id()` returns the stable entry ID.
    #[test]
    fn id_returns_entry_id() {
        let row = test_entry_row();
        let dir = make_dir_entry(&row);
        assert_eq!(dir.id(), 7);
    }

    // ==================== Property Tests ====================

    use proptest::prelude::*;

    use crate::proptest_config;

    proptest! {
        #![proptest_config(proptest_config::config())]

        /// All accessors round-trip arbitrary values from the entry row.
        #[test]
        fn accessors_round_trip(
            id in 1..=u32::MAX,
            created in any::<i64>(),
            modified in any::<i64>(),
            mode in any::<u32>(),
        ) {
            let row = EntryRow::new_directory(id, created, modified, mode);
            let dir = DirEntry {
                entry: &row,
                path: ArchivePath::from_bytes(b"test"),
                id,
            };
            prop_assert_eq!(dir.id(), id);
            prop_assert_eq!(dir.mode(), mode);
            prop_assert_eq!(dir.created_time(), created);
            prop_assert_eq!(dir.modified_time(), modified);
            prop_assert_eq!(dir.entry().entry_id.get(), id);
        }
    }
}
