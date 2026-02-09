//! Entry table row containing per-entry metadata.

use crate::EntryKind;
use zerocopy::byteorder::little_endian::{I64, U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

/// Entry table row (48 bytes) with per-entry metadata.
///
/// The entry table is a contiguous array of fixed-stride rows, one per entry.
/// Contains metadata but no paths. Rows are sorted by entry ID in ascending
/// order, enabling binary search.
///
/// # Layout
///
/// | Offset | Size | Field             | Description                         |
/// |--------|------|-------------------|-------------------------------------|
/// | 0      | 4    | Entry ID          | LE, unique within the archive       |
/// | 4      | 8    | Data block offset | LE, byte offset to data block (0 = none) |
/// | 12     | 8    | File size         | LE, original uncompressed size      |
/// | 20     | 8    | Block size        | LE, stored size (after compression) |
/// | 28     | 8    | Created time      | LE, i64, Unix epoch milliseconds    |
/// | 36     | 8    | Modified time     | LE, i64, Unix epoch milliseconds    |
/// | 44     | 4    | Mode              | LE, Unix permissions and file type  |
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub struct EntryRow {
    /// Entry ID, unique within the archive. ID 0 is reserved.
    pub entry_id: U32,
    /// Byte offset to the data block (0 = no data block).
    pub data_offset: U64,
    /// Original uncompressed file size in bytes.
    pub file_size: U64,
    /// Stored size in bytes (after compression, if any).
    pub block_size: U64,
    /// Creation time as Unix epoch milliseconds (signed).
    pub created_time: I64,
    /// Modification time as Unix epoch milliseconds (signed).
    pub modified_time: I64,
    /// Unix mode (file type in upper bits, permissions in lower bits).
    pub mode: U32,
}

impl EntryRow {
    /// Total size of an entry row in bytes.
    pub const SIZE: usize = 48;

    /// Creates a new `EntryRow` for a regular file.
    #[must_use]
    pub fn new_file(
        entry_id: u32,
        data_offset: u64,
        file_size: u64,
        block_size: u64,
        created_time: i64,
        modified_time: i64,
        mode: u32,
    ) -> Self {
        Self {
            entry_id: U32::new(entry_id),
            data_offset: U64::new(data_offset),
            file_size: U64::new(file_size),
            block_size: U64::new(block_size),
            created_time: I64::new(created_time),
            modified_time: I64::new(modified_time),
            mode: U32::new(mode),
        }
    }

    /// Creates a new `EntryRow` for a directory.
    ///
    /// Directories have no data block (`data_offset = 0`, `file_size = 0`,
    /// `block_size = 0`).
    #[must_use]
    pub fn new_directory(entry_id: u32, created_time: i64, modified_time: i64, mode: u32) -> Self {
        Self {
            entry_id: U32::new(entry_id),
            data_offset: U64::new(0),
            file_size: U64::new(0),
            block_size: U64::new(0),
            created_time: I64::new(created_time),
            modified_time: I64::new(modified_time),
            mode: U32::new(mode),
        }
    }

    /// Returns the entry kind based on the mode field.
    #[must_use]
    pub fn kind(&self) -> EntryKind {
        EntryKind::from_mode(self.mode.get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Structure must be exactly 48 bytes.
    #[test]
    fn size_is_48_bytes() {
        assert_eq!(std::mem::size_of::<EntryRow>(), EntryRow::SIZE);
        assert_eq!(EntryRow::SIZE, 48);
    }

    /// File constructor sets all fields correctly.
    #[test]
    fn new_file() {
        let row = EntryRow::new_file(
            1,
            4096,
            1024,
            1024,
            1_700_000_000_000,
            1_700_000_001_000,
            0o100644,
        );
        assert_eq!(row.entry_id.get(), 1);
        assert_eq!(row.data_offset.get(), 4096);
        assert_eq!(row.file_size.get(), 1024);
        assert_eq!(row.block_size.get(), 1024);
        assert_eq!(row.created_time.get(), 1_700_000_000_000);
        assert_eq!(row.modified_time.get(), 1_700_000_001_000);
        assert_eq!(row.mode.get(), 0o100644);
    }

    /// Directory constructor sets data fields to zero.
    #[test]
    fn new_directory() {
        let row = EntryRow::new_directory(3, 1_700_000_000_000, 1_700_000_001_000, 0o040755);
        assert_eq!(row.entry_id.get(), 3);
        assert_eq!(row.data_offset.get(), 0);
        assert_eq!(row.file_size.get(), 0);
        assert_eq!(row.block_size.get(), 0);
        assert_eq!(row.mode.get(), 0o040755);
    }

    /// Entry kind is correctly derived from mode.
    #[test]
    fn kind_from_mode() {
        let file = EntryRow::new_file(1, 4096, 100, 100, 0, 0, 0o100644);
        assert_eq!(file.kind(), EntryKind::File);

        let dir = EntryRow::new_directory(2, 0, 0, 0o040755);
        assert_eq!(dir.kind(), EntryKind::Directory);

        let symlink = EntryRow::new_file(3, 4096, 11, 11, 0, 0, 0o120777);
        assert_eq!(symlink.kind(), EntryKind::Symlink);
    }

    /// Negative timestamps are supported (pre-epoch dates).
    #[test]
    fn negative_timestamps() {
        let row = EntryRow::new_file(1, 4096, 100, 100, -1_000_000, -500_000, 0o100644);
        assert_eq!(row.created_time.get(), -1_000_000);
        assert_eq!(row.modified_time.get(), -500_000);
    }

    /// Round-trip serialization preserves all fields.
    #[test]
    fn roundtrip() {
        let row = EntryRow::new_file(
            42,
            8192,
            65536,
            32768,
            1_700_000_000_000,
            1_700_000_001_000,
            0o100755,
        );
        let bytes = row.as_bytes();
        assert_eq!(bytes.len(), EntryRow::SIZE);

        let restored = EntryRow::ref_from_bytes(bytes).unwrap();
        assert_eq!(restored.entry_id.get(), 42);
        assert_eq!(restored.data_offset.get(), 8192);
        assert_eq!(restored.file_size.get(), 65536);
        assert_eq!(restored.block_size.get(), 32768);
        assert_eq!(restored.created_time.get(), 1_700_000_000_000);
        assert_eq!(restored.modified_time.get(), 1_700_000_001_000);
        assert_eq!(restored.mode.get(), 0o100755);
    }
}
