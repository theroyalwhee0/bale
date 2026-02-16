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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ArchivePath;
    use crate::format::Crc;

    /// Creates a test file `EntryRow` with known values.
    fn test_entry_row() -> EntryRow {
        EntryRow::new_file(
            3,
            Crc::new(0xDEAD_BEEF),
            4096,
            1024,
            1024,
            1_700_000_000_000,
            1_700_000_001_000,
            0o100644,
        )
    }

    /// Creates a `FileEntry` referencing the given row.
    fn make_file_entry(row: &EntryRow) -> FileEntry<'_> {
        FileEntry {
            entry: row,
            path: ArchivePath::from_bytes(b"src/main.rs"),
            data: b"fn main() {}",
            id: 3,
        }
    }

    /// `data()` returns the file content bytes.
    #[test]
    fn data_returns_file_content() {
        let row = test_entry_row();
        let file = make_file_entry(&row);
        assert_eq!(file.data(), b"fn main() {}");
    }

    /// `size()` returns the uncompressed file size from the entry row.
    #[test]
    fn size_returns_file_size() {
        let row = test_entry_row();
        let file = make_file_entry(&row);
        assert_eq!(file.size(), 1024);
    }

    /// `path()` returns the file path.
    #[test]
    fn path_returns_archive_path() {
        let row = test_entry_row();
        let file = make_file_entry(&row);
        assert_eq!(file.path().as_bytes(), b"src/main.rs");
    }

    /// `mode()` returns the Unix mode from the entry row.
    #[test]
    fn mode_returns_unix_mode() {
        let row = test_entry_row();
        let file = make_file_entry(&row);
        assert_eq!(file.mode(), 0o100644);
    }

    /// `created_time()` returns the creation timestamp.
    #[test]
    fn created_time_returns_timestamp() {
        let row = test_entry_row();
        let file = make_file_entry(&row);
        assert_eq!(file.created_time(), 1_700_000_000_000);
    }

    /// `modified_time()` returns the modification timestamp.
    #[test]
    fn modified_time_returns_timestamp() {
        let row = test_entry_row();
        let file = make_file_entry(&row);
        assert_eq!(file.modified_time(), 1_700_000_001_000);
    }

    /// `entry()` returns a reference to the underlying entry row.
    #[test]
    fn entry_returns_row_reference() {
        let row = test_entry_row();
        let file = make_file_entry(&row);
        let returned = file.entry();
        assert_eq!(returned.entry_id.get(), 3);
        assert_eq!(returned.mode.get(), 0o100644);
    }

    /// `id()` returns the stable entry ID.
    #[test]
    fn id_returns_entry_id() {
        let row = test_entry_row();
        let file = make_file_entry(&row);
        assert_eq!(file.id(), 3);
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
            crc_val in any::<u32>(),
            offset in any::<u64>(),
            file_size in any::<u64>(),
            block_size in any::<u64>(),
            created in any::<i64>(),
            modified in any::<i64>(),
            mode in any::<u32>(),
        ) {
            let row = EntryRow::new_file(
                id, Crc::new(crc_val), offset,
                file_size, block_size, created, modified, mode,
            );
            let data = b"proptest data";
            let file = FileEntry {
                entry: &row,
                path: ArchivePath::from_bytes(b"test"),
                data: data.as_slice(),
                id,
            };
            prop_assert_eq!(file.id(), id);
            prop_assert_eq!(file.size(), file_size);
            prop_assert_eq!(file.mode(), mode);
            prop_assert_eq!(file.created_time(), created);
            prop_assert_eq!(file.modified_time(), modified);
            prop_assert_eq!(file.data(), data.as_slice());
            prop_assert_eq!(file.entry().entry_id.get(), id);
        }
    }
}
