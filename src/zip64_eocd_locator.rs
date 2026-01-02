use crate::BaleError;
use zerocopy::byteorder::little_endian::{U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

/// ZIP64 End of Central Directory Locator (20 bytes).
///
/// This structure appears between the ZIP64 EOCD and the standard EOCD,
/// pointing to the location of the ZIP64 EOCD record.
///
/// In bale archives, this structure is **always** present to maintain a
/// fixed trailer size.
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub struct Zip64EocdLocator {
    /// Magic signature: `0x07064b50`.
    pub signature: U32,
    /// Disk number containing the ZIP64 EOCD (always 0 for single-file).
    pub zip64_eocd_disk: U32,
    /// Offset to the ZIP64 EOCD record from start of archive.
    pub zip64_eocd_offset: U64,
    /// Total number of disks (always 1 for single-file archives).
    pub total_disks: U32,
}

impl Zip64EocdLocator {
    /// ZIP64 EOCD Locator signature: `0x07064b50`.
    pub const SIGNATURE: u32 = 0x07064b50;

    /// Size of this structure in bytes.
    pub const SIZE: usize = 20;

    /// Creates a new ZIP64 EOCD Locator.
    ///
    /// # Arguments
    ///
    /// * `zip64_eocd_offset` - Offset to the ZIP64 EOCD record from start of archive
    #[must_use]
    pub fn new(zip64_eocd_offset: u64) -> Self {
        Self {
            signature: U32::new(Self::SIGNATURE),
            zip64_eocd_disk: U32::new(0),
            zip64_eocd_offset: U64::new(zip64_eocd_offset),
            total_disks: U32::new(1),
        }
    }

    // ==================== Accessors ====================

    /// Returns the offset to the ZIP64 EOCD record.
    #[must_use]
    pub const fn zip64_eocd_offset(&self) -> u64 {
        self.zip64_eocd_offset.get()
    }

    // ==================== Validation ====================

    /// Validates the ZIP64 EOCD Locator signature.
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

    /// ZIP64 EOCD Locator is exactly 20 bytes.
    #[test]
    fn size_is_20_bytes() {
        assert_eq!(
            std::mem::size_of::<Zip64EocdLocator>(),
            Zip64EocdLocator::SIZE
        );
        assert_eq!(Zip64EocdLocator::SIZE, 20);
    }

    /// New locator has correct signature and defaults.
    #[test]
    fn new_has_correct_signature() {
        let locator = Zip64EocdLocator::new(0);
        assert!(locator.is_valid());
        assert_eq!(locator.signature.get(), Zip64EocdLocator::SIGNATURE);
        assert_eq!(locator.zip64_eocd_disk.get(), 0);
        assert_eq!(locator.total_disks.get(), 1);
    }

    /// Locator with offset roundtrips correctly.
    #[test]
    fn roundtrip_with_offset() {
        let locator = Zip64EocdLocator::new(5_000_000_000);
        let bytes = locator.as_bytes();
        assert_eq!(bytes.len(), Zip64EocdLocator::SIZE);

        let restored = Zip64EocdLocator::ref_from_bytes(bytes).unwrap();
        assert!(restored.is_valid());
        assert_eq!(restored.zip64_eocd_offset(), 5_000_000_000);
    }

    /// Accessor returns correct value.
    #[test]
    fn accessor() {
        let locator = Zip64EocdLocator::new(12345);
        assert_eq!(locator.zip64_eocd_offset(), 12345);
    }

    /// Invalid signature fails is_valid().
    #[test]
    fn invalid_signature_fails_validation() {
        let mut locator = Zip64EocdLocator::new(0);
        locator.signature = U32::new(0x12345678);
        assert!(!locator.is_valid());
    }

    /// validated() returns Ok for valid locator.
    #[test]
    fn validated_returns_ok_for_valid() {
        let locator = Zip64EocdLocator::new(0);
        assert!(locator.validated().is_ok());
    }

    /// validated() returns Err for invalid signature.
    #[test]
    fn validated_returns_err_for_invalid() {
        let mut locator = Zip64EocdLocator::new(0);
        locator.signature = U32::new(0xDEADBEEF);
        let err = locator.validated().unwrap_err();
        assert!(matches!(
            err,
            BaleError::InvalidSignature {
                expected: 0x07064b50,
                found: 0xDEADBEEF
            }
        ));
    }
}
