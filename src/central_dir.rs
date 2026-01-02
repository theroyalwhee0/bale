use zerocopy::byteorder::little_endian::{U16, U32};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

use crate::DosDateTime;

/// Central Directory File Header (46 bytes fixed, followed by filename).
///
/// These headers appear after all local file entries and before the EOCD.
/// For bale archives, the filename is always `path_size` bytes (null-padded).
///
/// # Layout
///
/// Uses `#[repr(C)]` with `Unaligned` because all fields are zerocopy
/// little-endian types (`U16`, `U32`), which are 1-byte aligned. ZIP headers
/// can appear at any byte offset in an archive, so unaligned access is required.
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub struct CentralDirectoryHeader {
    /// Magic signature: `0x02014b50`.
    pub signature: U32,
    /// Version made by (high byte = OS, low byte = ZIP version).
    pub version_made_by: U16,
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
    /// Length of file comment (always 0 for bale).
    pub comment_length: U16,
    /// Disk number where file starts (always 0 for bale).
    pub disk_start: U16,
    /// Internal file attributes (0 for binary).
    pub internal_attrs: U16,
    /// External file attributes (Unix permissions in high bytes).
    pub external_attrs: U32,
    /// Offset to corresponding Local File Header.
    pub local_header_offset: U32,
}

impl CentralDirectoryHeader {
    /// Central Directory signature: `0x02014b50`.
    pub const SIGNATURE: u32 = 0x02014b50;

    /// Size of the fixed header portion in bytes.
    pub const SIZE: usize = 46;

    /// Version made by: Unix (3) in high byte, ZIP 1.0 (10) in low byte.
    ///
    /// Bale archives always report Unix as the creation OS, regardless of the
    /// host platform. This ensures consistent external attributes interpretation
    /// (Unix permissions in upper 16 bits of `external_attrs`).
    const VERSION_MADE_BY_UNIX: u16 = (3 << 8) | 10;

    /// Version needed to extract for STORE method.
    const VERSION_STORE: u16 = 10;

    /// Compression method: STORE (no compression).
    const COMPRESSION_STORE: u16 = 0;

    /// Returns the total stride (header + filename) for a given path size.
    ///
    /// Uses saturating addition to avoid overflow on pathological inputs.
    #[must_use]
    pub const fn stride(path_size: usize) -> usize {
        Self::SIZE.saturating_add(path_size)
    }

    /// Creates a new `CentralDirectoryHeader` for an uncompressed file.
    ///
    /// # Arguments
    ///
    /// * `size` - Size of the file data in bytes
    /// * `crc32` - CRC-32 checksum of the file data
    /// * `mtime` - Modification time in MS-DOS format
    /// * `local_offset` - Offset to the Local File Header
    /// * `unix_mode` - Unix file permissions (e.g., 0o644)
    /// * `path_size` - Length of the filename field
    #[must_use]
    pub fn new(
        size: u32,
        crc32: u32,
        mtime: DosDateTime,
        local_offset: u32,
        unix_mode: u32,
        path_size: u16,
    ) -> Self {
        // External attributes: Unix mode in upper 16 bits.
        let external_attrs = unix_mode << 16;

        Self {
            signature: U32::new(Self::SIGNATURE),
            version_made_by: U16::new(Self::VERSION_MADE_BY_UNIX),
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
            comment_length: U16::new(0),
            disk_start: U16::new(0),
            internal_attrs: U16::new(0),
            external_attrs: U32::new(external_attrs),
            local_header_offset: U32::new(local_offset),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixed header portion is exactly 46 bytes per ZIP spec.
    #[test]
    fn header_size_is_46_bytes() {
        assert_eq!(
            std::mem::size_of::<CentralDirectoryHeader>(),
            CentralDirectoryHeader::SIZE
        );
    }

    /// Stride includes header (46) plus filename field.
    #[test]
    fn stride_is_header_plus_path() {
        assert_eq!(CentralDirectoryHeader::stride(256), 46 + 256);
        assert_eq!(CentralDirectoryHeader::stride(128), 46 + 128);
    }

    /// New headers must have the correct ZIP signature.
    #[test]
    fn new_header_has_correct_signature() {
        let header =
            CentralDirectoryHeader::new(100, 0x12345678, DosDateTime::default(), 0, 0o644, 256);
        assert_eq!(header.signature.get(), CentralDirectoryHeader::SIGNATURE);
    }

    /// Unix permissions are stored in the upper 16 bits of external_attrs.
    #[test]
    fn unix_mode_in_external_attrs() {
        let header = CentralDirectoryHeader::new(100, 0, DosDateTime::default(), 0, 0o755, 256);
        // 0o755 = 0x1ED, shifted left 16 bits = 0x01ED0000
        assert_eq!(header.external_attrs.get(), 0o755 << 16);
    }

    /// Header can be serialized and deserialized without data loss.
    #[test]
    fn roundtrip() {
        let mtime = DosDateTime::from_date_time_parts(0x58CF, 0x6955);
        let header = CentralDirectoryHeader::new(1024, 0xDEADBEEF, mtime, 4096, 0o644, 256);
        let bytes = header.as_bytes();
        assert_eq!(bytes.len(), CentralDirectoryHeader::SIZE);

        let restored = CentralDirectoryHeader::ref_from_bytes(bytes).unwrap();
        assert_eq!(restored.signature.get(), CentralDirectoryHeader::SIGNATURE);
        assert_eq!(restored.uncompressed_size.get(), 1024);
        assert_eq!(restored.crc32.get(), 0xDEADBEEF);
        assert_eq!(restored.local_header_offset.get(), 4096);
        assert_eq!(restored.filename_length.get(), 256);
    }
}
