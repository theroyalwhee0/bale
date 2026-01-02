use crate::BaleError;
use zerocopy::byteorder::little_endian::{U16, U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

/// ZIP64 End of Central Directory record (56 bytes).
///
/// This structure appears before the standard EOCD in ZIP64 archives and
/// contains 64-bit versions of the central directory metadata fields.
///
/// In bale archives, this structure is **always** present to maintain a
/// fixed trailer size. The standard EOCD contains the actual values when
/// they fit, or overflow markers (`0xFFFF`/`0xFFFFFFFF`) when they don't.
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub struct Zip64Eocd {
    /// Magic signature: `0x06064b50`.
    pub signature: U32,
    /// Size of the remaining record (44 bytes for standard ZIP64).
    pub record_size: U64,
    /// Version made by (high byte = OS, low byte = ZIP version).
    pub version_made_by: U16,
    /// Minimum version needed to extract.
    pub version_needed: U16,
    /// Disk number (always 0 for single-file archives).
    pub disk_number: U32,
    /// Disk where central directory starts (always 0).
    pub cd_start_disk: U32,
    /// Number of central directory entries on this disk.
    pub cd_entries_disk: U64,
    /// Total number of central directory entries.
    pub cd_entries_total: U64,
    /// Size of the central directory in bytes.
    pub cd_size: U64,
    /// Offset to the start of the central directory.
    pub cd_offset: U64,
}

impl Zip64Eocd {
    /// ZIP64 EOCD signature: `0x06064b50`.
    pub const SIGNATURE: u32 = 0x06064b50;

    /// Size of this structure in bytes.
    pub const SIZE: usize = 56;

    /// Size of the record excluding signature and record_size fields.
    /// This is 56 - 4 (signature) - 8 (record_size) = 44 bytes.
    const RECORD_DATA_SIZE: u64 = 44;

    /// Version made by: Unix (3) in high byte, ZIP 4.5 (45) in low byte.
    /// ZIP 4.5 is required for ZIP64 support.
    const VERSION_MADE_BY_UNIX: u16 = (3 << 8) | 45;

    /// Version needed to extract for ZIP64.
    const VERSION_NEEDED_ZIP64: u16 = 45;

    /// Creates a new ZIP64 EOCD with the given parameters.
    ///
    /// # Arguments
    ///
    /// * `entry_count` - Number of entries in the archive
    /// * `cd_size` - Size of the central directory in bytes
    /// * `cd_offset` - Offset to the start of the central directory
    #[must_use]
    pub fn new(entry_count: u64, cd_size: u64, cd_offset: u64) -> Self {
        Self {
            signature: U32::new(Self::SIGNATURE),
            record_size: U64::new(Self::RECORD_DATA_SIZE),
            version_made_by: U16::new(Self::VERSION_MADE_BY_UNIX),
            version_needed: U16::new(Self::VERSION_NEEDED_ZIP64),
            disk_number: U32::new(0),
            cd_start_disk: U32::new(0),
            cd_entries_disk: U64::new(entry_count),
            cd_entries_total: U64::new(entry_count),
            cd_size: U64::new(cd_size),
            cd_offset: U64::new(cd_offset),
        }
    }

    // ==================== Accessors ====================

    /// Returns the total number of central directory entries.
    #[must_use]
    pub const fn cd_entries(&self) -> u64 {
        self.cd_entries_total.get()
    }

    /// Returns the size of the central directory in bytes.
    #[must_use]
    pub const fn cd_size(&self) -> u64 {
        self.cd_size.get()
    }

    /// Returns the offset to the start of the central directory.
    #[must_use]
    pub const fn cd_offset(&self) -> u64 {
        self.cd_offset.get()
    }

    // ==================== Validation ====================

    /// Validates the ZIP64 EOCD signature and record size.
    ///
    /// The record size must be at least 44 bytes (the standard ZIP64 EOCD
    /// data size). The ZIP spec allows extensible data after these 44 bytes.
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        self.signature.get() == Self::SIGNATURE && self.record_size.get() >= Self::RECORD_DATA_SIZE
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

    /// ZIP64 EOCD is exactly 56 bytes.
    #[test]
    fn size_is_56_bytes() {
        assert_eq!(std::mem::size_of::<Zip64Eocd>(), Zip64Eocd::SIZE);
        assert_eq!(Zip64Eocd::SIZE, 56);
    }

    /// New ZIP64 EOCD has correct signature and record size.
    #[test]
    fn new_has_correct_signature() {
        let eocd = Zip64Eocd::new(0, 0, 0);
        assert!(eocd.is_valid());
        assert_eq!(eocd.signature.get(), Zip64Eocd::SIGNATURE);
        assert_eq!(eocd.record_size.get(), 44);
    }

    /// ZIP64 EOCD with values roundtrips correctly.
    #[test]
    fn roundtrip_with_values() {
        let eocd = Zip64Eocd::new(100_000, 1_000_000_000, 2_000_000_000);
        let bytes = eocd.as_bytes();
        assert_eq!(bytes.len(), Zip64Eocd::SIZE);

        let restored = Zip64Eocd::ref_from_bytes(bytes).unwrap();
        assert!(restored.is_valid());
        assert_eq!(restored.cd_entries(), 100_000);
        assert_eq!(restored.cd_size(), 1_000_000_000);
        assert_eq!(restored.cd_offset(), 2_000_000_000);
    }

    /// Accessors return correct values.
    #[test]
    fn accessors() {
        let eocd = Zip64Eocd::new(42, 1000, 2000);
        assert_eq!(eocd.cd_entries(), 42);
        assert_eq!(eocd.cd_size(), 1000);
        assert_eq!(eocd.cd_offset(), 2000);
    }

    /// Invalid signature fails is_valid().
    #[test]
    fn invalid_signature_fails_validation() {
        let mut eocd = Zip64Eocd::new(0, 0, 0);
        eocd.signature = U32::new(0x12345678);
        assert!(!eocd.is_valid());
    }

    /// Invalid record_size fails is_valid().
    #[test]
    fn invalid_record_size_fails_validation() {
        let mut eocd = Zip64Eocd::new(0, 0, 0);
        eocd.record_size = U64::new(43); // Less than required 44
        assert!(!eocd.is_valid());
    }

    /// record_size >= 44 passes validation (allows extensible data).
    #[test]
    fn larger_record_size_passes_validation() {
        let mut eocd = Zip64Eocd::new(0, 0, 0);
        eocd.record_size = U64::new(100); // Larger than 44 is allowed
        assert!(eocd.is_valid());
    }

    /// validated() returns Ok for valid ZIP64 EOCD.
    #[test]
    fn validated_returns_ok_for_valid() {
        let eocd = Zip64Eocd::new(0, 0, 0);
        assert!(eocd.validated().is_ok());
    }

    /// validated() returns Err for invalid signature.
    #[test]
    fn validated_returns_err_for_invalid() {
        let mut eocd = Zip64Eocd::new(0, 0, 0);
        eocd.signature = U32::new(0xDEADBEEF);
        let err = eocd.validated().unwrap_err();
        assert!(matches!(
            err,
            BaleError::InvalidSignature {
                expected: 0x06064b50,
                found: 0xDEADBEEF
            }
        ));
    }

    /// Large values beyond u32 range work correctly.
    #[test]
    fn large_values() {
        let large_offset: u64 = 5_000_000_000; // > u32::MAX
        let large_entries: u64 = 100_000;
        let large_size: u64 = 10_000_000_000;

        let eocd = Zip64Eocd::new(large_entries, large_size, large_offset);
        assert_eq!(eocd.cd_entries(), large_entries);
        assert_eq!(eocd.cd_size(), large_size);
        assert_eq!(eocd.cd_offset(), large_offset);
    }
}
