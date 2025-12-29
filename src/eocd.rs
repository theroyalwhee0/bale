use zerocopy::byteorder::little_endian::{U16, U32};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

/// End of Central Directory record (zip-compatible, 22 bytes).
///
/// This structure appears at the end of a zip/bale archive and contains
/// metadata about the central directory location and entry count.
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub struct Eocd {
    /// Magic signature: 0x06054b50.
    pub signature: U32,
    /// Disk number (always 0 for single-file archives).
    pub disk_number: U16,
    /// Disk where central directory starts (always 0).
    pub cd_start_disk: U16,
    /// Number of central directory entries on this disk.
    pub cd_entries_disk: U16,
    /// Total number of central directory entries.
    pub cd_entries_total: U16,
    /// Size of the central directory in bytes.
    pub cd_size: U32,
    /// Offset to the start of the central directory.
    pub cd_offset: U32,
    /// Length of the archive comment (always 0 for bale).
    pub comment_length: U16,
}

impl Eocd {
    /// EOCD signature: 0x06054b50.
    pub const SIGNATURE: u32 = 0x06054b50;

    /// Size of the EOCD structure in bytes.
    pub const SIZE: usize = 22;

    /// Creates a new empty EOCD for an archive with no entries.
    pub fn empty() -> Self {
        Self {
            signature: U32::new(Self::SIGNATURE),
            disk_number: U16::new(0),
            cd_start_disk: U16::new(0),
            cd_entries_disk: U16::new(0),
            cd_entries_total: U16::new(0),
            cd_size: U32::new(0),
            cd_offset: U32::new(0),
            comment_length: U16::new(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eocd_size_is_22_bytes() {
        assert_eq!(std::mem::size_of::<Eocd>(), Eocd::SIZE);
    }

    #[test]
    fn eocd_roundtrip() {
        let eocd = Eocd::empty();
        let bytes = eocd.as_bytes();
        assert_eq!(bytes.len(), Eocd::SIZE);

        let restored = Eocd::ref_from_bytes(bytes).unwrap();
        assert_eq!(restored.signature.get(), Eocd::SIGNATURE);
    }
}
