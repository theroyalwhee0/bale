//! Unified 256-byte trailer combining all end-of-archive structures.

use super::{BaleEocd, Eocd, Zip64Eocd, Zip64EocdLocator};
use crate::BaleError;
use zerocopy::{FromBytes, IntoBytes};

/// Unified 256-byte trailer containing all end-of-archive structures.
///
/// The trailer layout is:
/// - ZIP64 EOCD (56 bytes)
/// - ZIP64 EOCD Locator (20 bytes)
/// - EOCD (22 bytes)
/// - BaleEocd (158 bytes)
///
/// This struct provides a unified API for reading and writing the trailer,
/// with convenience methods that automatically resolve ZIP64 overflow markers.
#[derive(Debug, Clone, Copy)]
pub struct Trailer {
    /// ZIP64 End of Central Directory record.
    pub zip64_eocd: Zip64Eocd,
    /// ZIP64 EOCD Locator.
    pub zip64_locator: Zip64EocdLocator,
    /// Standard End of Central Directory record.
    pub eocd: Eocd,
    /// Bale-specific EOCD extension.
    pub bale_eocd: BaleEocd,
}

impl Trailer {
    /// Size of the complete trailer in bytes.
    pub const SIZE: usize = 256;

    /// Parses the trailer from the end of archive bytes.
    ///
    /// Extracts the last 256 bytes and parses all four trailer structures.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The archive is too small to contain a trailer
    /// - Any signature is invalid
    /// - Any structure fails validation
    pub fn from_archive_bytes(bytes: &[u8]) -> Result<Self, BaleError> {
        // Check minimum size.
        if bytes.len() < Self::SIZE {
            return Err(BaleError::TooSmall {
                size: bytes.len() as u64,
                minimum: Self::SIZE as u64,
            });
        }

        // Extract last 256 bytes. Safe because we verified length above.
        let trailer_start = bytes.len() - Self::SIZE;
        let trailer: [u8; Self::SIZE] = bytes[trailer_start..]
            .try_into()
            .ok()
            .ok_or_else(|| BaleError::Corrupted("failed to extract trailer bytes".into()))?;

        Self::from_bytes(&trailer)
    }

    /// Parses the 256-byte trailer from a fixed-size byte array.
    ///
    /// # Errors
    ///
    /// Returns an error if any signature is invalid or any structure
    /// fails validation.
    pub fn from_bytes(bytes: &[u8; Self::SIZE]) -> Result<Self, BaleError> {
        // Parse ZIP64 EOCD (first 56 bytes).
        let zip64_eocd = Zip64Eocd::ref_from_bytes(&bytes[..Zip64Eocd::SIZE])
            .map_err(|e| BaleError::Corrupted(format!("invalid ZIP64 EOCD: {e}")))?;
        zip64_eocd.validated()?;

        // Parse ZIP64 EOCD Locator (next 20 bytes).
        let locator_start = Zip64Eocd::SIZE;
        let locator_end = locator_start + Zip64EocdLocator::SIZE;
        let zip64_locator = Zip64EocdLocator::ref_from_bytes(&bytes[locator_start..locator_end])
            .map_err(|e| BaleError::Corrupted(format!("invalid ZIP64 EOCD Locator: {e}")))?;
        zip64_locator.validated()?;

        // Parse EOCD (next 22 bytes).
        let eocd_start = locator_end;
        let eocd_end = eocd_start + Eocd::SIZE;
        let eocd = Eocd::ref_from_bytes(&bytes[eocd_start..eocd_end])
            .map_err(|e| BaleError::Corrupted(format!("invalid EOCD: {e}")))?;
        eocd.validated()?;

        // Parse BaleEocd (final 158 bytes).
        let bale_start = eocd_end;
        let bale_eocd = BaleEocd::ref_from_bytes(&bytes[bale_start..])
            .map_err(|e| BaleError::Corrupted(format!("invalid BaleEocd: {e}")))?;
        bale_eocd.validated()?;

        Ok(Self {
            zip64_eocd: *zip64_eocd,
            zip64_locator: *zip64_locator,
            eocd: *eocd,
            bale_eocd: *bale_eocd,
        })
    }

    /// Creates a new trailer for writing.
    ///
    /// # Arguments
    ///
    /// * `entry_count` - Number of entries in the archive
    /// * `cd_size` - Size of the central directory in bytes
    /// * `cd_offset` - Offset to the start of the central directory
    /// * `zip64_eocd_offset` - Offset to the ZIP64 EOCD from start of archive
    /// * `bale_eocd` - The bale-specific EOCD extension
    #[must_use]
    pub fn new(
        entry_count: u64,
        cd_size: u64,
        cd_offset: u64,
        zip64_eocd_offset: u64,
        bale_eocd: BaleEocd,
    ) -> Self {
        // Create ZIP64 EOCD with true 64-bit values.
        let zip64_eocd = Zip64Eocd::new(entry_count, cd_size, cd_offset);

        // Create ZIP64 EOCD Locator.
        let zip64_locator = Zip64EocdLocator::new(zip64_eocd_offset);

        // Create EOCD with overflow markers if values exceed limits.
        let eocd_entries = if entry_count > u64::from(u16::MAX) {
            u16::MAX
        } else {
            entry_count as u16
        };
        let eocd_cd_size = if cd_size > u64::from(u32::MAX) {
            u32::MAX
        } else {
            cd_size as u32
        };
        let eocd_cd_offset = if cd_offset > u64::from(u32::MAX) {
            u32::MAX
        } else {
            cd_offset as u32
        };
        let eocd = Eocd::new_with_comment(
            eocd_entries,
            eocd_cd_size,
            eocd_cd_offset,
            BaleEocd::SIZE as u16,
        );

        Self {
            zip64_eocd,
            zip64_locator,
            eocd,
            bale_eocd,
        }
    }

    /// Serializes the trailer to a 256-byte array.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0u8; Self::SIZE];
        let mut offset = 0;

        // Write ZIP64 EOCD.
        bytes[offset..offset + Zip64Eocd::SIZE].copy_from_slice(self.zip64_eocd.as_bytes());
        offset += Zip64Eocd::SIZE;

        // Write ZIP64 EOCD Locator.
        bytes[offset..offset + Zip64EocdLocator::SIZE]
            .copy_from_slice(self.zip64_locator.as_bytes());
        offset += Zip64EocdLocator::SIZE;

        // Write EOCD.
        bytes[offset..offset + Eocd::SIZE].copy_from_slice(self.eocd.as_bytes());
        offset += Eocd::SIZE;

        // Write BaleEocd.
        bytes[offset..].copy_from_slice(self.bale_eocd.as_bytes());

        bytes
    }

    // ==================== Validation ====================

    /// Validates all four signatures in one call.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.zip64_eocd.is_valid()
            && self.zip64_locator.is_valid()
            && self.eocd.is_valid()
            && self.bale_eocd.is_valid()
    }

    /// Validates and returns a reference or an error.
    ///
    /// # Errors
    ///
    /// Returns `BaleError::Corrupted` if any validation check fails.
    pub fn validated(&self) -> Result<&Self, BaleError> {
        self.zip64_eocd.validated()?;
        self.zip64_locator.validated()?;
        self.eocd.validated()?;
        self.bale_eocd.validated()?;
        Ok(self)
    }

    // ==================== Central Directory Accessors ====================

    /// Returns the entry count, resolving ZIP64 overflow markers.
    ///
    /// If `eocd.cd_entries == 0xFFFF`, returns the value from `zip64_eocd`.
    /// In bale archives, ZIP64 values are always authoritative.
    #[must_use]
    pub fn entry_count(&self) -> u64 {
        if self.eocd.cd_entries() == u16::MAX {
            self.zip64_eocd.cd_entries()
        } else {
            u64::from(self.eocd.cd_entries())
        }
    }

    /// Returns the CD offset, resolving ZIP64 overflow markers.
    ///
    /// If `eocd.cd_offset == 0xFFFFFFFF`, returns the value from `zip64_eocd`.
    /// In bale archives, ZIP64 values are always authoritative.
    #[must_use]
    pub fn cd_offset(&self) -> u64 {
        if self.eocd.cd_offset() == u32::MAX {
            self.zip64_eocd.cd_offset()
        } else {
            u64::from(self.eocd.cd_offset())
        }
    }

    /// Returns the CD size, resolving ZIP64 overflow markers.
    ///
    /// If `eocd.cd_size == 0xFFFFFFFF`, returns the value from `zip64_eocd`.
    /// In bale archives, ZIP64 values are always authoritative.
    #[must_use]
    pub fn cd_size(&self) -> u64 {
        if self.eocd.cd_size() == u32::MAX {
            self.zip64_eocd.cd_size()
        } else {
            u64::from(self.eocd.cd_size())
        }
    }

    // ==================== BaleEocd Convenience Accessors ====================

    /// Returns the alignment in bytes.
    ///
    /// # Panics
    ///
    /// Panics if `alignment_pow2` is invalid. This cannot happen for trailers
    /// created via [`from_bytes()`](Self::from_bytes) or
    /// [`from_archive_bytes()`](Self::from_archive_bytes) since validation
    /// occurs on construction.
    #[must_use]
    pub fn alignment(&self) -> u32 {
        self.bale_eocd.alignment().expect("alignment_pow2 invalid")
    }

    /// Returns the maximum path size.
    #[must_use]
    pub fn path_size(&self) -> u16 {
        self.bale_eocd.path_size()
    }

    /// Returns the format version as (major, minor, patch).
    #[must_use]
    pub fn version(&self) -> (u8, u8, u8) {
        self.bale_eocd.version()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trailer must be exactly 256 bytes.
    #[test]
    fn size_is_256_bytes() {
        assert_eq!(Trailer::SIZE, 256);
        assert_eq!(
            Trailer::SIZE,
            Zip64Eocd::SIZE + Zip64EocdLocator::SIZE + Eocd::SIZE + BaleEocd::SIZE
        );
    }

    /// Trailer roundtrips through to_bytes/from_bytes.
    #[test]
    fn roundtrip() {
        let bale_eocd = BaleEocd::new();
        let trailer = Trailer::new(42, 1000, 5000, 6000, bale_eocd);

        let bytes = trailer.to_bytes();
        let parsed = Trailer::from_bytes(&bytes).expect("roundtrip should succeed");

        assert_eq!(parsed.entry_count(), 42);
        assert_eq!(parsed.cd_size(), 1000);
        assert_eq!(parsed.cd_offset(), 5000);
        assert_eq!(parsed.zip64_locator.zip64_eocd_offset(), 6000);
        assert_eq!(parsed.path_size(), BaleEocd::DEFAULT_PATH_SIZE);
        assert_eq!(parsed.alignment(), BaleEocd::DEFAULT_ALIGNMENT);
    }

    /// Overflow markers are set correctly when values exceed limits.
    #[test]
    fn overflow_markers() {
        let bale_eocd = BaleEocd::new();
        let large_count = u64::from(u16::MAX) + 100;
        let large_offset = u64::from(u32::MAX) + 100;
        let large_size = u64::from(u32::MAX) + 200;

        let trailer = Trailer::new(large_count, large_size, large_offset, 0, bale_eocd);

        // EOCD should have overflow markers.
        assert_eq!(trailer.eocd.cd_entries(), u16::MAX);
        assert_eq!(trailer.eocd.cd_offset(), u32::MAX);
        assert_eq!(trailer.eocd.cd_size(), u32::MAX);

        // ZIP64 should have actual values.
        assert_eq!(trailer.zip64_eocd.cd_entries(), large_count);
        assert_eq!(trailer.zip64_eocd.cd_offset(), large_offset);
        assert_eq!(trailer.zip64_eocd.cd_size(), large_size);

        // Convenience methods should resolve to ZIP64 values.
        assert_eq!(trailer.entry_count(), large_count);
        assert_eq!(trailer.cd_offset(), large_offset);
        assert_eq!(trailer.cd_size(), large_size);
    }

    /// Values below overflow threshold are stored in EOCD directly.
    #[test]
    fn no_overflow_uses_eocd_values() {
        let bale_eocd = BaleEocd::new();
        let trailer = Trailer::new(100, 5000, 10000, 0, bale_eocd);

        // EOCD should have actual values (not overflow markers).
        assert_eq!(trailer.eocd.cd_entries(), 100);
        assert_eq!(trailer.eocd.cd_offset(), 10000);
        assert_eq!(trailer.eocd.cd_size(), 5000);

        // Convenience methods should return the same values.
        assert_eq!(trailer.entry_count(), 100);
        assert_eq!(trailer.cd_offset(), 10000);
        assert_eq!(trailer.cd_size(), 5000);
    }

    /// Invalid signature causes from_bytes to fail.
    #[test]
    fn invalid_zip64_eocd_signature() {
        let bale_eocd = BaleEocd::new();
        let trailer = Trailer::new(0, 0, 0, 0, bale_eocd);
        let mut bytes = trailer.to_bytes();

        // Corrupt ZIP64 EOCD signature (first 4 bytes).
        bytes[0] = 0xFF;

        let result = Trailer::from_bytes(&bytes);
        assert!(result.is_err());
    }

    /// Invalid locator signature causes from_bytes to fail.
    #[test]
    fn invalid_zip64_locator_signature() {
        let bale_eocd = BaleEocd::new();
        let trailer = Trailer::new(0, 0, 0, 0, bale_eocd);
        let mut bytes = trailer.to_bytes();

        // Corrupt ZIP64 EOCD Locator signature.
        let locator_offset = Zip64Eocd::SIZE;
        bytes[locator_offset] = 0xFF;

        let result = Trailer::from_bytes(&bytes);
        assert!(result.is_err());
    }

    /// Invalid EOCD signature causes from_bytes to fail.
    #[test]
    fn invalid_eocd_signature() {
        let bale_eocd = BaleEocd::new();
        let trailer = Trailer::new(0, 0, 0, 0, bale_eocd);
        let mut bytes = trailer.to_bytes();

        // Corrupt EOCD signature.
        let eocd_offset = Zip64Eocd::SIZE + Zip64EocdLocator::SIZE;
        bytes[eocd_offset] = 0xFF;

        let result = Trailer::from_bytes(&bytes);
        assert!(result.is_err());
    }

    /// Invalid BaleEocd magic causes from_bytes to fail.
    #[test]
    fn invalid_bale_eocd_magic() {
        let bale_eocd = BaleEocd::new();
        let trailer = Trailer::new(0, 0, 0, 0, bale_eocd);
        let mut bytes = trailer.to_bytes();

        // Corrupt BaleEocd magic.
        let bale_offset = Zip64Eocd::SIZE + Zip64EocdLocator::SIZE + Eocd::SIZE;
        bytes[bale_offset] = 0xFF;

        let result = Trailer::from_bytes(&bytes);
        assert!(result.is_err());
    }

    /// from_archive_bytes extracts the last 256 bytes.
    #[test]
    fn from_archive_bytes_extracts_trailer() {
        let bale_eocd = BaleEocd::new();
        let trailer = Trailer::new(7, 100, 200, 300, bale_eocd);
        let trailer_bytes = trailer.to_bytes();

        // Prepend some garbage data.
        let mut archive = vec![0xAA; 1000];
        archive.extend_from_slice(&trailer_bytes);

        let parsed = Trailer::from_archive_bytes(&archive).expect("should parse trailer");
        assert_eq!(parsed.entry_count(), 7);
        assert_eq!(parsed.cd_size(), 100);
        assert_eq!(parsed.cd_offset(), 200);
    }

    /// from_archive_bytes fails if archive is too small.
    #[test]
    fn from_archive_bytes_too_small() {
        let bytes = [0u8; 100];
        let result = Trailer::from_archive_bytes(&bytes);
        assert!(matches!(result, Err(BaleError::TooSmall { .. })));
    }

    /// is_valid returns true for valid trailer.
    #[test]
    fn is_valid_true_for_valid_trailer() {
        let bale_eocd = BaleEocd::new();
        let trailer = Trailer::new(0, 0, 0, 0, bale_eocd);
        assert!(trailer.is_valid());
    }

    /// validated returns Ok for valid trailer.
    #[test]
    fn validated_ok_for_valid_trailer() {
        let bale_eocd = BaleEocd::new();
        let trailer = Trailer::new(0, 0, 0, 0, bale_eocd);
        assert!(trailer.validated().is_ok());
    }

    /// BaleEocd convenience accessors work correctly.
    #[test]
    fn bale_eocd_accessors() {
        let bale_eocd = BaleEocd::new_with_options(8192, 512).expect("valid options");
        let trailer = Trailer::new(0, 0, 0, 0, bale_eocd);

        assert_eq!(trailer.alignment(), 8192);
        assert_eq!(trailer.path_size(), 512);
        assert_eq!(trailer.version(), BaleEocd::CURRENT_VERSION);
    }
}
