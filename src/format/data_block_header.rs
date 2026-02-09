//! Data block header preceding each data block in the archive.

use zerocopy::byteorder::little_endian::{U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

/// Data block header (32 bytes) at the start of each data block.
///
/// Each entry with content (regular files and symlinks) has one data block.
/// The data block starts at an alignment boundary and consists of this header
/// followed by the stored data bytes.
///
/// # Layout
///
/// | Offset | Size | Field              | Description                       |
/// |--------|------|--------------------|-----------------------------------|
/// | 0      | 4    | Entry ID           | LE, matches entry table row       |
/// | 4      | 8    | File size          | LE, original uncompressed size    |
/// | 12     | 8    | Block size         | LE, stored size (after compression) |
/// | 20     | 4    | CRC-32             | Checksum of the stored data bytes |
/// | 24     | 1    | Compression method | 0 = none (stored)                 |
/// | 25     | 7    | Reserved           | Must be zero                      |
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub struct DataBlockHeader {
    /// Entry ID matching the corresponding entry table row.
    pub entry_id: U32,
    /// Original uncompressed file size in bytes.
    pub file_size: U64,
    /// Stored size in bytes (after compression, if any).
    pub block_size: U64,
    /// CRC-32 checksum of the stored data bytes.
    pub crc32: U32,
    /// Compression method (0 = none/stored).
    pub compression: u8,
    /// Reserved for future use. Must be zero.
    pub reserved: [u8; Self::RESERVED_SIZE],
}

impl DataBlockHeader {
    /// Total size of the data block header in bytes.
    pub const SIZE: usize = 32;

    /// Size of the reserved field in bytes.
    const RESERVED_SIZE: usize = 7;

    /// Compression method: stored (no compression).
    pub const COMPRESSION_NONE: u8 = 0;

    /// Creates a new `DataBlockHeader` for uncompressed data.
    ///
    /// Sets `file_size` and `block_size` to the same value since there is
    /// no compression.
    #[must_use]
    pub fn new(entry_id: u32, size: u64, crc32: u32) -> Self {
        Self {
            entry_id: U32::new(entry_id),
            file_size: U64::new(size),
            block_size: U64::new(size),
            crc32: U32::new(crc32),
            compression: Self::COMPRESSION_NONE,
            reserved: [0u8; Self::RESERVED_SIZE],
        }
    }

    /// Creates a new `DataBlockHeader` with explicit file and block sizes.
    ///
    /// Use this when `file_size` and `block_size` differ (e.g., with compression).
    #[must_use]
    pub fn new_with_sizes(
        entry_id: u32,
        file_size: u64,
        block_size: u64,
        crc32: u32,
        compression: u8,
    ) -> Self {
        Self {
            entry_id: U32::new(entry_id),
            file_size: U64::new(file_size),
            block_size: U64::new(block_size),
            crc32: U32::new(crc32),
            compression,
            reserved: [0u8; Self::RESERVED_SIZE],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Structure must be exactly 32 bytes.
    #[test]
    fn size_is_32_bytes() {
        assert_eq!(
            std::mem::size_of::<DataBlockHeader>(),
            DataBlockHeader::SIZE
        );
        assert_eq!(DataBlockHeader::SIZE, 32);
    }

    /// Uncompressed constructor sets file_size == block_size.
    #[test]
    fn new_uncompressed() {
        let header = DataBlockHeader::new(1, 1024, 0xDEAD_BEEF);
        assert_eq!(header.entry_id.get(), 1);
        assert_eq!(header.file_size.get(), 1024);
        assert_eq!(header.block_size.get(), 1024);
        assert_eq!(header.crc32.get(), 0xDEAD_BEEF);
        assert_eq!(header.compression, DataBlockHeader::COMPRESSION_NONE);
    }

    /// Explicit sizes constructor allows different file and block sizes.
    #[test]
    fn new_with_sizes() {
        let header = DataBlockHeader::new_with_sizes(2, 2048, 1024, 0x1234_5678, 1);
        assert_eq!(header.entry_id.get(), 2);
        assert_eq!(header.file_size.get(), 2048);
        assert_eq!(header.block_size.get(), 1024);
        assert_eq!(header.crc32.get(), 0x1234_5678);
        assert_eq!(header.compression, 1);
    }

    /// Reserved bytes are all zero.
    #[test]
    fn reserved_is_zero() {
        let header = DataBlockHeader::new(1, 100, 0);
        assert!(header.reserved.iter().all(|&b| b == 0));
    }

    /// Round-trip serialization preserves all fields.
    #[test]
    fn roundtrip() {
        let header = DataBlockHeader::new(42, 65536, 0xCAFE_BABE);
        let bytes = header.as_bytes();
        assert_eq!(bytes.len(), DataBlockHeader::SIZE);

        let restored = DataBlockHeader::ref_from_bytes(bytes).unwrap();
        assert_eq!(restored.entry_id.get(), 42);
        assert_eq!(restored.file_size.get(), 65536);
        assert_eq!(restored.block_size.get(), 65536);
        assert_eq!(restored.crc32.get(), 0xCAFE_BABE);
        assert_eq!(restored.compression, DataBlockHeader::COMPRESSION_NONE);
    }

    /// Round-trip with compression preserves different file and block sizes.
    #[test]
    fn roundtrip_compressed() {
        let header = DataBlockHeader::new_with_sizes(7, 10000, 5000, 0xABCD_EF01, 2);
        let bytes = header.as_bytes();
        let restored = DataBlockHeader::ref_from_bytes(bytes).unwrap();
        assert_eq!(restored.entry_id.get(), 7);
        assert_eq!(restored.file_size.get(), 10000);
        assert_eq!(restored.block_size.get(), 5000);
        assert_eq!(restored.crc32.get(), 0xABCD_EF01);
        assert_eq!(restored.compression, 2);
    }
}
