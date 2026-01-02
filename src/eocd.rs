use crate::BaleError;
use zerocopy::byteorder::little_endian::{U16, U32};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

/// End of Central Directory record (ZIP-compatible, 22 bytes).
///
/// This structure appears at the end of a ZIP/bale archive and contains
/// metadata about the central directory location and entry count.
///
/// In bale archives, this is followed by the [`BaleEocd`](crate::BaleEocd)
/// comment field.
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub struct Eocd {
    /// Magic signature: `0x06054b50`.
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
    /// Length of the archive comment following this structure.
    pub comment_length: U16,
}

impl Eocd {
    /// EOCD signature: 0x06054b50.
    pub const SIGNATURE: u32 = 0x06054b50;

    /// Size of the EOCD structure in bytes.
    pub const SIZE: usize = 22;

    /// Creates a new empty EOCD for an archive with no entries and no comment.
    #[must_use]
    pub fn empty() -> Self {
        Self::new_with_comment(0, 0, 0, 0)
    }

    /// Creates a new EOCD with the given parameters and no comment.
    ///
    /// # Arguments
    ///
    /// * `entry_count` - Number of entries in the archive
    /// * `cd_size` - Size of the central directory in bytes
    /// * `cd_offset` - Offset to the start of the central directory
    #[must_use]
    pub fn new(entry_count: u16, cd_size: u32, cd_offset: u32) -> Self {
        Self::new_with_comment(entry_count, cd_size, cd_offset, 0)
    }

    /// Creates a new EOCD with a comment of the specified length.
    ///
    /// # Arguments
    ///
    /// * `entry_count` - Number of entries in the archive
    /// * `cd_size` - Size of the central directory in bytes
    /// * `cd_offset` - Offset to the start of the central directory
    /// * `comment_length` - Length of the comment following the EOCD
    #[must_use]
    pub fn new_with_comment(
        entry_count: u16,
        cd_size: u32,
        cd_offset: u32,
        comment_length: u16,
    ) -> Self {
        Self {
            signature: U32::new(Self::SIGNATURE),
            disk_number: U16::new(0),
            cd_start_disk: U16::new(0),
            cd_entries_disk: U16::new(entry_count),
            cd_entries_total: U16::new(entry_count),
            cd_size: U32::new(cd_size),
            cd_offset: U32::new(cd_offset),
            comment_length: U16::new(comment_length),
        }
    }

    // ==================== Accessors ====================

    /// Returns the total number of central directory entries.
    #[must_use]
    pub const fn cd_entries(&self) -> u16 {
        self.cd_entries_total.get()
    }

    /// Returns the size of the central directory in bytes.
    #[must_use]
    pub const fn cd_size(&self) -> u32 {
        self.cd_size.get()
    }

    /// Returns the offset to the start of the central directory.
    #[must_use]
    pub const fn cd_offset(&self) -> u32 {
        self.cd_offset.get()
    }

    /// Returns the length of the archive comment.
    #[must_use]
    pub const fn comment_length(&self) -> u16 {
        self.comment_length.get()
    }

    // ==================== Validation ====================

    /// Validates the EOCD signature.
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        self.signature.get() == Self::SIGNATURE
    }

    /// Validates the structure and returns a reference or an error.
    ///
    /// # Errors
    ///
    /// Returns `BaleError::InvalidSignature` if the signature doesn't match.
    pub fn validated(&self) -> Result<&Self, BaleError> {
        if self.is_valid() {
            Ok(self)
        } else {
            Err(BaleError::InvalidSignature {
                expected: Self::SIGNATURE,
                found: self.signature.get(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// EOCD is exactly 22 bytes per ZIP spec.
    #[test]
    fn size_is_22_bytes() {
        assert_eq!(std::mem::size_of::<Eocd>(), Eocd::SIZE);
        assert_eq!(Eocd::SIZE, 22);
    }

    /// Empty EOCD has correct defaults.
    #[test]
    fn empty_has_correct_defaults() {
        let eocd = Eocd::empty();
        assert!(eocd.is_valid());
        assert_eq!(eocd.cd_entries(), 0);
        assert_eq!(eocd.cd_size(), 0);
        assert_eq!(eocd.cd_offset(), 0);
        assert_eq!(eocd.comment_length(), 0);
    }

    /// EOCD with values roundtrips correctly.
    #[test]
    fn roundtrip_with_values() {
        let eocd = Eocd::new_with_comment(100, 4096, 65536, 234);
        let bytes = eocd.as_bytes();
        assert_eq!(bytes.len(), Eocd::SIZE);

        let restored = Eocd::ref_from_bytes(bytes).unwrap();
        assert!(restored.is_valid());
        assert_eq!(restored.cd_entries(), 100);
        assert_eq!(restored.cd_size(), 4096);
        assert_eq!(restored.cd_offset(), 65536);
        assert_eq!(restored.comment_length(), 234);
    }

    /// Accessors return correct values.
    #[test]
    fn accessors() {
        let eocd = Eocd::new_with_comment(42, 1000, 2000, 128);
        assert_eq!(eocd.cd_entries(), 42);
        assert_eq!(eocd.cd_size(), 1000);
        assert_eq!(eocd.cd_offset(), 2000);
        assert_eq!(eocd.comment_length(), 128);
    }

    /// Invalid signature fails is_valid().
    #[test]
    fn invalid_signature_fails_validation() {
        let mut eocd = Eocd::empty();
        eocd.signature = U32::new(0x12345678);
        assert!(!eocd.is_valid());
    }

    /// validated() returns Ok for valid EOCD.
    #[test]
    fn validated_returns_ok_for_valid() {
        let eocd = Eocd::empty();
        assert!(eocd.validated().is_ok());
    }

    /// validated() returns Err for invalid signature.
    #[test]
    fn validated_returns_err_for_invalid() {
        let mut eocd = Eocd::empty();
        eocd.signature = U32::new(0xDEADBEEF);
        let err = eocd.validated().unwrap_err();
        assert!(matches!(
            err,
            BaleError::InvalidSignature {
                expected: 0x06054b50,
                found: 0xDEADBEEF
            }
        ));
    }
}
