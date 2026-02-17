//! Entry table row containing per-entry metadata.

use crate::EntryKind;
use crate::format::Crc;
use zerocopy::byteorder::little_endian::{I64, U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

/// Entry table row (64 bytes) with per-entry metadata.
///
/// The entry table is a contiguous array of fixed-stride rows, one per entry.
/// Contains metadata but no paths. Rows are sorted by entry ID in ascending
/// order, enabling binary search.
///
/// # Layout
///
/// | Offset | Size | Field             | Description                              |
/// |--------|------|-------------------|------------------------------------------|
/// | 0      | 4    | Entry ID          | LE, unique within archive (0 = tombstone)|
/// | 4      | 4    | CRC-32C           | LE, checksum of stored data bytes        |
/// | 8      | 8    | Data block offset | LE, byte offset to data (0 = none)       |
/// | 16     | 8    | File size         | LE, original uncompressed size           |
/// | 24     | 8    | Block size        | LE, stored size (after compression)      |
/// | 32     | 8    | Created time      | LE, i64, Unix epoch milliseconds         |
/// | 40     | 8    | Modified time     | LE, i64, Unix epoch milliseconds         |
/// | 48     | 4    | Mode              | LE, Unix permissions and file type       |
/// | 52     | 1    | Compression       | 0 = none (stored)                        |
/// | 53     | 1    | Flags             | u8, bitfield (see Entry Flags)           |
/// | 54     | 10   | Reserved          | Must be zero                             |
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub struct EntryRow {
    /// Entry ID, unique within the archive. ID 0 is reserved (tombstone).
    pub entry_id: U32,
    /// CRC-32C checksum of the stored data bytes.
    pub crc32c: U32,
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
    /// Compression method (0 = none/stored).
    pub compression: u8,
    /// Entry flags bitfield. All bits reserved in v1.0.0.
    pub flags: u8,
    /// Reserved for future use. Must be zero.
    pub reserved: [u8; Self::RESERVED_SIZE],
}

impl EntryRow {
    /// Total size of an entry row in bytes.
    pub const SIZE: usize = 64;

    /// Size of the reserved field in bytes.
    const RESERVED_SIZE: usize = 10;

    /// Compression method: stored (no compression).
    pub const COMPRESSION_NONE: u8 = 0;

    /// Creates a new `EntryRow` for a regular file.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new_file(
        entry_id: u32,
        crc: Crc,
        data_offset: u64,
        file_size: u64,
        block_size: u64,
        created_time: i64,
        modified_time: i64,
        mode: u32,
    ) -> Self {
        Self {
            entry_id: U32::new(entry_id),
            crc32c: U32::new(crc.to_u32()),
            data_offset: U64::new(data_offset),
            file_size: U64::new(file_size),
            block_size: U64::new(block_size),
            created_time: I64::new(created_time),
            modified_time: I64::new(modified_time),
            mode: U32::new(mode),
            compression: Self::COMPRESSION_NONE,
            flags: 0,
            reserved: [0u8; Self::RESERVED_SIZE],
        }
    }

    /// Creates a new `EntryRow` for a directory.
    ///
    /// Directories have no data block (`data_offset = 0`, `file_size = 0`,
    /// `block_size = 0`) and no CRC.
    #[must_use]
    pub fn new_directory(entry_id: u32, created_time: i64, modified_time: i64, mode: u32) -> Self {
        Self {
            entry_id: U32::new(entry_id),
            crc32c: U32::new(Crc::NONE.to_u32()),
            data_offset: U64::new(0),
            file_size: U64::new(0),
            block_size: U64::new(0),
            created_time: I64::new(created_time),
            modified_time: I64::new(modified_time),
            mode: U32::new(mode),
            compression: Self::COMPRESSION_NONE,
            flags: 0,
            reserved: [0u8; Self::RESERVED_SIZE],
        }
    }

    /// Returns the CRC-32 checksum for this entry.
    #[must_use]
    pub fn crc(&self) -> Crc {
        Crc::new(self.crc32c.get())
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

    /// Structure must be exactly 64 bytes.
    #[test]
    fn size_is_64_bytes() {
        assert_eq!(std::mem::size_of::<EntryRow>(), EntryRow::SIZE);
        assert_eq!(EntryRow::SIZE, 64);
    }

    /// File constructor sets all fields correctly.
    #[test]
    fn new_file() {
        let row = EntryRow::new_file(
            1,
            Crc::new(0xDEAD_BEEF),
            4096,
            1024,
            1024,
            1_700_000_000_000,
            1_700_000_001_000,
            0o100644,
        );
        assert_eq!(row.entry_id.get(), 1);
        assert_eq!(row.crc().get(), Some(0xDEAD_BEEF));
        assert_eq!(row.data_offset.get(), 4096);
        assert_eq!(row.file_size.get(), 1024);
        assert_eq!(row.block_size.get(), 1024);
        assert_eq!(row.created_time.get(), 1_700_000_000_000);
        assert_eq!(row.modified_time.get(), 1_700_000_001_000);
        assert_eq!(row.mode.get(), 0o100644);
        assert_eq!(row.compression, EntryRow::COMPRESSION_NONE);
        assert_eq!(row.flags, 0);
        assert!(row.reserved.iter().all(|&b| b == 0));
    }

    /// Directory constructor sets data fields to zero.
    #[test]
    fn new_directory() {
        let row = EntryRow::new_directory(3, 1_700_000_000_000, 1_700_000_001_000, 0o040755);
        assert_eq!(row.entry_id.get(), 3);
        assert_eq!(row.crc().get(), None);
        assert_eq!(row.data_offset.get(), 0);
        assert_eq!(row.file_size.get(), 0);
        assert_eq!(row.block_size.get(), 0);
        assert_eq!(row.mode.get(), 0o040755);
        assert_eq!(row.compression, EntryRow::COMPRESSION_NONE);
        assert_eq!(row.flags, 0);
    }

    /// Entry kind is correctly derived from mode.
    #[test]
    fn kind_from_mode() {
        let file = EntryRow::new_file(1, Crc::NONE, 4096, 100, 100, 0, 0, 0o100644);
        assert_eq!(file.kind(), EntryKind::File);

        let dir = EntryRow::new_directory(2, 0, 0, 0o040755);
        assert_eq!(dir.kind(), EntryKind::Directory);

        let symlink = EntryRow::new_file(3, Crc::NONE, 4096, 11, 11, 0, 0, 0o120777);
        assert_eq!(symlink.kind(), EntryKind::Symlink);
    }

    /// Negative timestamps are supported (pre-epoch dates).
    #[test]
    fn negative_timestamps() {
        let row = EntryRow::new_file(1, Crc::NONE, 4096, 100, 100, -1_000_000, -500_000, 0o100644);
        assert_eq!(row.created_time.get(), -1_000_000);
        assert_eq!(row.modified_time.get(), -500_000);
    }

    /// Round-trip serialization preserves all fields.
    #[test]
    fn roundtrip() {
        let row = EntryRow::new_file(
            42,
            Crc::new(0xCAFE_BABE),
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
        assert_eq!(restored.crc().get(), Some(0xCAFE_BABE));
        assert_eq!(restored.data_offset.get(), 8192);
        assert_eq!(restored.file_size.get(), 65536);
        assert_eq!(restored.block_size.get(), 32768);
        assert_eq!(restored.created_time.get(), 1_700_000_000_000);
        assert_eq!(restored.modified_time.get(), 1_700_000_001_000);
        assert_eq!(restored.mode.get(), 0o100755);
        assert_eq!(restored.compression, EntryRow::COMPRESSION_NONE);
        assert_eq!(restored.flags, 0);
    }

    /// Maximum entry ID (`u32::MAX`) round-trips through serialization.
    #[test]
    fn boundary_entry_id_max() {
        let row = EntryRow::new_file(u32::MAX, Crc::NONE, 0, 0, 0, 0, 0, 0o100644);
        let bytes = row.as_bytes();
        let restored = EntryRow::ref_from_bytes(bytes).unwrap();
        assert_eq!(restored.entry_id.get(), u32::MAX);
    }

    /// Maximum `u64` values for `file_size` and `block_size` round-trip.
    #[test]
    fn boundary_file_size_max() {
        let row = EntryRow::new_file(1, Crc::NONE, 0, u64::MAX, u64::MAX, 0, 0, 0o100644);
        let bytes = row.as_bytes();
        let restored = EntryRow::ref_from_bytes(bytes).unwrap();
        assert_eq!(restored.file_size.get(), u64::MAX);
        assert_eq!(restored.block_size.get(), u64::MAX);
    }

    /// Extreme `i64` timestamp values (`MIN` and `MAX`) round-trip.
    #[test]
    fn boundary_timestamps_min_max() {
        let row = EntryRow::new_file(1, Crc::NONE, 0, 0, 0, i64::MIN, i64::MAX, 0o100644);
        let bytes = row.as_bytes();
        let restored = EntryRow::ref_from_bytes(bytes).unwrap();
        assert_eq!(restored.created_time.get(), i64::MIN);
        assert_eq!(restored.modified_time.get(), i64::MAX);
    }

    /// Maximum `u64` data offset round-trips through serialization.
    #[test]
    fn boundary_data_offset_max() {
        let row = EntryRow::new_file(1, Crc::NONE, u64::MAX, 0, 0, 0, 0, 0o100644);
        let bytes = row.as_bytes();
        let restored = EntryRow::ref_from_bytes(bytes).unwrap();
        assert_eq!(restored.data_offset.get(), u64::MAX);
    }

    /// Non-default compression and flags survive serialization.
    #[test]
    fn compression_and_flags_roundtrip() {
        let mut row = EntryRow::new_file(1, Crc::NONE, 0, 0, 0, 0, 0, 0o100644);
        row.compression = 7;
        row.flags = 0b1010_0101;
        let bytes = row.as_bytes();
        let restored = EntryRow::ref_from_bytes(bytes).unwrap();
        assert_eq!(restored.compression, 7);
        assert_eq!(restored.flags, 0b1010_0101);
    }

    /// Non-zero reserved bytes survive serialization.
    #[test]
    fn reserved_field_roundtrip() {
        let mut row = EntryRow::new_file(1, Crc::NONE, 0, 0, 0, 0, 0, 0o100644);
        row.reserved = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let bytes = row.as_bytes();
        let restored = EntryRow::ref_from_bytes(bytes).unwrap();
        assert_eq!(restored.reserved, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    }

    // ==================== Property Tests ====================

    use crate::proptest_config;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(proptest_config::config())]

        /// Arbitrary 64-byte input never panics when interpreted as an EntryRow.
        ///
        /// Exercises `ref_from_bytes`, `kind`, `crc`, and all accessor methods
        /// on random byte patterns.
        #[test]
        fn fuzz_entry_row_parsing(data in prop::collection::vec(any::<u8>(), 64..=64)) {
            let row = EntryRow::ref_from_bytes(&data).unwrap();
            let _ = row.kind();
            let _ = row.crc();
            let _ = row.entry_id.get();
            let _ = row.data_offset.get();
            let _ = row.file_size.get();
            let _ = row.block_size.get();
            let _ = row.created_time.get();
            let _ = row.modified_time.get();
            let _ = row.mode.get();
            let _ = row.compression;
            let _ = row.flags;
        }
    }
}
