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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ArchivePath;
    use crate::format::Crc;

    /// Creates a test symlink `EntryRow` with known values.
    fn test_entry_row() -> EntryRow {
        EntryRow::new_file(
            5,
            Crc::NONE,
            4096,
            11,
            11,
            1_700_000_000_000,
            1_700_000_001_000,
            0o120777,
        )
    }

    /// Creates a `SymlinkEntry` referencing the given row with a UTF-8 target.
    fn make_symlink_entry(row: &EntryRow) -> SymlinkEntry<'_> {
        SymlinkEntry {
            entry: row,
            path: ArchivePath::from_bytes(b"lib/latest"),
            target: b"lib/v2.0.1",
            id: 5,
        }
    }

    /// `target_bytes()` returns the raw target bytes.
    #[test]
    fn target_bytes_returns_raw_target() {
        let row = test_entry_row();
        let sym = make_symlink_entry(&row);
        assert_eq!(sym.target_bytes(), b"lib/v2.0.1");
    }

    /// `target()` returns `Some` for valid UTF-8 targets.
    #[test]
    fn target_returns_some_for_valid_utf8() {
        let row = test_entry_row();
        let sym = make_symlink_entry(&row);
        assert_eq!(sym.target(), Some("lib/v2.0.1"));
    }

    /// `target()` returns `None` for invalid UTF-8 targets.
    #[test]
    fn target_returns_none_for_invalid_utf8() {
        let row = test_entry_row();
        let sym = SymlinkEntry {
            entry: &row,
            path: ArchivePath::from_bytes(b"link"),
            target: &[0xFF, 0xFE],
            id: 5,
        };
        assert_eq!(sym.target(), None);
    }

    /// `path()` returns the symlink path.
    #[test]
    fn path_returns_archive_path() {
        let row = test_entry_row();
        let sym = make_symlink_entry(&row);
        assert_eq!(sym.path().as_bytes(), b"lib/latest");
    }

    /// `mode()` returns the Unix mode from the entry row.
    #[test]
    fn mode_returns_unix_mode() {
        let row = test_entry_row();
        let sym = make_symlink_entry(&row);
        assert_eq!(sym.mode(), 0o120777);
    }

    /// `created_time()` returns the creation timestamp.
    #[test]
    fn created_time_returns_timestamp() {
        let row = test_entry_row();
        let sym = make_symlink_entry(&row);
        assert_eq!(sym.created_time(), 1_700_000_000_000);
    }

    /// `modified_time()` returns the modification timestamp.
    #[test]
    fn modified_time_returns_timestamp() {
        let row = test_entry_row();
        let sym = make_symlink_entry(&row);
        assert_eq!(sym.modified_time(), 1_700_000_001_000);
    }

    /// `entry()` returns a reference to the underlying entry row.
    #[test]
    fn entry_returns_row_reference() {
        let row = test_entry_row();
        let sym = make_symlink_entry(&row);
        let returned = sym.entry();
        assert_eq!(returned.entry_id.get(), 5);
        assert_eq!(returned.mode.get(), 0o120777);
    }

    /// `id()` returns the stable entry ID.
    #[test]
    fn id_returns_entry_id() {
        let row = test_entry_row();
        let sym = make_symlink_entry(&row);
        assert_eq!(sym.id(), 5);
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
            let row = EntryRow::new_file(
                id, Crc::NONE, 4096, 10, 10, created, modified, mode,
            );
            let sym = SymlinkEntry {
                entry: &row,
                path: ArchivePath::from_bytes(b"test"),
                target: b"target",
                id,
            };
            prop_assert_eq!(sym.id(), id);
            prop_assert_eq!(sym.mode(), mode);
            prop_assert_eq!(sym.created_time(), created);
            prop_assert_eq!(sym.modified_time(), modified);
            prop_assert_eq!(sym.entry().entry_id.get(), id);
        }

        /// `target()` returns `Some` for valid UTF-8 strings.
        #[test]
        fn target_valid_utf8(target in "[a-zA-Z0-9/_.-]{1,50}") {
            let row = EntryRow::new_file(
                1, Crc::NONE, 4096, target.len() as u64,
                target.len() as u64, 0, 0, 0o120777,
            );
            let target_bytes = target.as_bytes();
            let sym = SymlinkEntry {
                entry: &row,
                path: ArchivePath::from_bytes(b"link"),
                target: target_bytes,
                id: 1,
            };
            prop_assert_eq!(sym.target(), Some(target.as_str()));
            prop_assert_eq!(sym.target_bytes(), target_bytes);
        }

        /// `target()` returns `None` for invalid UTF-8 bytes.
        ///
        /// Uses lone continuation bytes (0x80..=0xBF) which are always
        /// invalid without a preceding multi-byte start byte.
        #[test]
        fn target_invalid_utf8(
            byte in 0x80u8..=0xBFu8,
            padding in prop::collection::vec(0x80u8..=0xBFu8, 0..=4),
        ) {
            let mut target = vec![byte];
            target.extend_from_slice(&padding);
            let row = EntryRow::new_file(
                1, Crc::NONE, 4096, target.len() as u64,
                target.len() as u64, 0, 0, 0o120777,
            );
            let sym = SymlinkEntry {
                entry: &row,
                path: ArchivePath::from_bytes(b"link"),
                target: &target,
                id: 1,
            };
            prop_assert!(sym.target().is_none());
            prop_assert_eq!(sym.target_bytes(), target.as_slice());
        }
    }
}
