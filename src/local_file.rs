use crate::DosDateTime;
use zerocopy::byteorder::little_endian::{U16, U32};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

/// Local File Header (30 bytes fixed, followed by filename).
///
/// This structure precedes each file's data in the archive.
/// For bale archives, the filename is always `path_size` bytes (null-padded).
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub struct LocalFileHeader {
    /// Magic signature: `0x04034b50`.
    pub signature: U32,
    /// Minimum version needed to extract (10 for STORE).
    pub version_needed: U16,
    /// General purpose bit flags (0 for bale).
    pub flags: U16,
    /// Compression method (0 = STORE).
    pub compression: U16,
    /// MS-DOS modification time.
    pub mod_time: U16,
    /// MS-DOS modification date.
    pub mod_date: U16,
    /// CRC-32 checksum of uncompressed data.
    pub crc32: U32,
    /// Compressed size (same as uncompressed for STORE).
    pub compressed_size: U32,
    /// Uncompressed size.
    pub uncompressed_size: U32,
    /// Length of filename field (always `PATH_SIZE` for bale).
    pub filename_length: U16,
    /// Length of extra field (always 0 for bale).
    pub extra_length: U16,
}

impl LocalFileHeader {
    /// Local File Header signature: `0x04034b50`.
    pub const SIGNATURE: u32 = 0x04034b50;

    /// Size of the fixed header portion in bytes.
    pub const SIZE: usize = 30;

    /// Version needed to extract for STORE method.
    const VERSION_STORE: u16 = 10;

    /// Compression method: STORE (no compression).
    const COMPRESSION_STORE: u16 = 0;

    /// Returns the total stride (header + filename) for a given path size.
    #[must_use]
    pub const fn stride(path_size: usize) -> usize {
        Self::SIZE + path_size
    }

    /// Creates a new `LocalFileHeader` for an uncompressed file.
    ///
    /// # Arguments
    ///
    /// * `size` - Size of the file data in bytes
    /// * `crc32` - CRC-32 checksum of the file data
    /// * `mtime` - Modification time in MS-DOS format
    /// * `path_size` - Length of the filename field
    #[must_use]
    pub fn new(size: u32, crc32: u32, mtime: DosDateTime, path_size: u16) -> Self {
        Self {
            signature: U32::new(Self::SIGNATURE),
            version_needed: U16::new(Self::VERSION_STORE),
            flags: U16::new(0),
            compression: U16::new(Self::COMPRESSION_STORE),
            mod_time: U16::new(mtime.time),
            mod_date: U16::new(mtime.date),
            crc32: U32::new(crc32),
            compressed_size: U32::new(size),
            uncompressed_size: U32::new(size),
            filename_length: U16::new(path_size),
            extra_length: U16::new(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixed header portion is exactly 30 bytes per ZIP spec.
    #[test]
    fn header_size_is_30_bytes() {
        assert_eq!(
            std::mem::size_of::<LocalFileHeader>(),
            LocalFileHeader::SIZE
        );
    }

    /// Stride includes header (30) plus filename field.
    #[test]
    fn stride_is_header_plus_path() {
        assert_eq!(LocalFileHeader::stride(256), 30 + 256);
        assert_eq!(LocalFileHeader::stride(128), 30 + 128);
    }

    /// New headers must have the correct ZIP signature.
    #[test]
    fn new_header_has_correct_signature() {
        let header = LocalFileHeader::new(100, 0x12345678, DosDateTime::default(), 256);
        assert_eq!(header.signature.get(), LocalFileHeader::SIGNATURE);
    }

    /// Header can be serialized and deserialized without data loss.
    #[test]
    fn roundtrip() {
        let mtime = DosDateTime::new(0x58CF, 0x6955);
        let header = LocalFileHeader::new(1024, 0xDEADBEEF, mtime, 256);
        let bytes = header.as_bytes();
        assert_eq!(bytes.len(), LocalFileHeader::SIZE);

        let restored = LocalFileHeader::ref_from_bytes(bytes).unwrap();
        assert_eq!(restored.signature.get(), LocalFileHeader::SIGNATURE);
        assert_eq!(restored.uncompressed_size.get(), 1024);
        assert_eq!(restored.crc32.get(), 0xDEADBEEF);
        assert_eq!(restored.mod_date.get(), 0x58CF);
        assert_eq!(restored.mod_time.get(), 0x6955);
        assert_eq!(restored.filename_length.get(), 256);
    }
}
