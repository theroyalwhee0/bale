use crate::{BaleError, Eocd};
use zerocopy::byteorder::little_endian::{U16, U32};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

/// Bale-specific EOCD extension stored as the ZIP comment field.
///
/// This 234-byte structure follows the standard 22-byte EOCD, making the
/// combined trailer exactly 256 bytes for efficient single-read access.
///
/// Contains archive-level configuration:
/// - Version information for format compatibility
/// - Alignment power (2^N) for file data placement
/// - Path size limit for fixed-stride entries
///
/// The structure remains valid even for ZIP64 archives since ZIP64 records
/// are placed before the standard EOCD.
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub struct BaleEocd {
    /// Magic signature: "BALE" (0x454C4142 little-endian).
    pub magic: U32,
    /// Major version number.
    pub version_major: u8,
    /// Minor version number.
    pub version_minor: u8,
    /// Patch version number.
    pub version_patch: u8,
    /// Alignment as power of 2 (e.g., 12 means 2^12 = 4096 bytes).
    pub alignment_pow2: u8,
    /// Maximum path size in bytes (1-2048).
    pub path_size: U16,
    /// Reserved for future use.
    ///
    /// Writers must set this to all zeros. Readers should ignore non-zero bytes
    /// for forward compatibility with future format extensions.
    pub reserved: [u8; Self::RESERVED_SIZE],
}

impl BaleEocd {
    /// Magic signature bytes: "BALE".
    pub const MAGIC: u32 = 0x454C_4142; // "BALE" as little-endian u32

    /// Total size of this structure in bytes (256 - 22 = 234).
    pub const SIZE: usize = 234;

    /// Combined size of EOCD + BaleEocd for single-read access.
    pub const COMBINED_SIZE: usize = Eocd::SIZE + Self::SIZE; // 256

    /// Size of the reserved field.
    const RESERVED_SIZE: usize = Self::SIZE - 10; // 224 bytes

    /// Minimum allowed path size.
    pub const MIN_PATH_SIZE: u16 = 1;

    /// Maximum allowed path size.
    pub const MAX_PATH_SIZE: u16 = 2048;

    /// Maximum alignment power (2^24 = 16 MB).
    pub(crate) const MAX_ALIGNMENT_POW2: u8 = 24;

    /// Current format version (major, minor, patch).
    ///
    /// Version compatibility policy:
    /// - **Major**: Breaking format change. Readers should refuse incompatible major versions.
    /// - **Minor**: Backward-compatible additions. Older readers can safely read newer minor versions.
    /// - **Patch**: Implementation-only changes with no format impact.
    ///
    /// Currently, version checking is not enforced; all versions are accepted.
    pub const CURRENT_VERSION: (u8, u8, u8) = (0, 1, 0);

    /// Creates a new `BaleEocd` with default settings (4096 alignment, 256 path size).
    #[must_use]
    pub fn new() -> Self {
        let (major, minor, patch) = Self::CURRENT_VERSION;
        Self {
            magic: U32::new(Self::MAGIC),
            version_major: major,
            version_minor: minor,
            version_patch: patch,
            alignment_pow2: 12, // 2^12 = 4096
            path_size: U16::new(256),
            reserved: [0u8; Self::RESERVED_SIZE],
        }
    }

    /// Creates a new `BaleEocd` with the given parameters.
    ///
    /// # Arguments
    ///
    /// * `alignment` - Alignment in bytes (must be a power of 2)
    /// * `path_size` - Maximum path size (MIN_PATH_SIZE..=MAX_PATH_SIZE)
    ///
    /// # Errors
    ///
    /// - Returns `BaleError::InvalidAlignment` if `alignment` is not a power of 2,
    ///   is zero, or exceeds 16 MB (2^24).
    /// - Returns `BaleError::InvalidPathSize` if `path_size` is not in range 1..=2048.
    pub fn new_with_options(alignment: u32, path_size: u16) -> Result<Self, BaleError> {
        let max_alignment = 1u32 << Self::MAX_ALIGNMENT_POW2;
        if !alignment.is_power_of_two() || alignment > max_alignment {
            return Err(BaleError::InvalidAlignment(alignment));
        }
        if !(Self::MIN_PATH_SIZE..=Self::MAX_PATH_SIZE).contains(&path_size) {
            return Err(BaleError::InvalidPathSize(path_size));
        }
        let alignment_pow2 = alignment.trailing_zeros() as u8;
        let (major, minor, patch) = Self::CURRENT_VERSION;
        Ok(Self {
            magic: U32::new(Self::MAGIC),
            version_major: major,
            version_minor: minor,
            version_patch: patch,
            alignment_pow2,
            path_size: U16::new(path_size),
            reserved: [0u8; Self::RESERVED_SIZE],
        })
    }

    /// Returns the alignment in bytes.
    ///
    /// Returns 1 if `alignment_pow2` exceeds [`MAX_ALIGNMENT_POW2`](Self::MAX_ALIGNMENT_POW2).
    /// Callers should use [`is_valid()`](Self::is_valid) to detect invalid values.
    #[must_use]
    pub const fn alignment(&self) -> u32 {
        if self.alignment_pow2 > Self::MAX_ALIGNMENT_POW2 {
            return 1;
        }
        1 << self.alignment_pow2
    }

    /// Returns the maximum path size.
    #[must_use]
    pub fn path_size(&self) -> u16 {
        self.path_size.get()
    }

    /// Returns the version as a tuple (major, minor, patch).
    #[must_use]
    pub const fn version(&self) -> (u8, u8, u8) {
        (self.version_major, self.version_minor, self.version_patch)
    }

    /// Validates the structure fields.
    ///
    /// Checks:
    /// - Magic signature is "BALE"
    /// - `alignment_pow2` is within valid range (≤ 24)
    /// - `path_size` is in range 1..=2048
    ///
    /// Note: The `reserved` field is NOT checked. Non-zero reserved bytes are
    /// silently ignored for forward compatibility with future format extensions.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.magic.get() == Self::MAGIC
            && self.alignment_pow2 <= Self::MAX_ALIGNMENT_POW2
            && (Self::MIN_PATH_SIZE..=Self::MAX_PATH_SIZE).contains(&self.path_size.get())
    }
}

impl Default for BaleEocd {
    /// Returns a `BaleEocd` with default settings (4096 alignment, 256 path size).
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Structure must be exactly 234 bytes (256 - 22 for EOCD).
    #[test]
    fn size_is_234_bytes() {
        assert_eq!(std::mem::size_of::<BaleEocd>(), BaleEocd::SIZE);
        assert_eq!(BaleEocd::SIZE, 234);
    }

    /// Combined EOCD + BaleEocd is exactly 256 bytes.
    #[test]
    fn combined_size_is_256_bytes() {
        assert_eq!(BaleEocd::COMBINED_SIZE, 256);
        assert_eq!(Eocd::SIZE + BaleEocd::SIZE, 256);
    }

    /// Default settings use 4096 alignment and 256 path size.
    #[test]
    fn default_settings() {
        let bale = BaleEocd::new();
        assert_eq!(bale.alignment(), 4096);
        assert_eq!(bale.path_size(), 256);
        assert!(bale.is_valid());
    }

    /// Alignment is correctly encoded as power of 2.
    #[test]
    fn alignment_encoding() {
        let bale = BaleEocd::new_with_options(4096, 256).unwrap();
        assert_eq!(bale.alignment_pow2, 12); // 2^12 = 4096
        assert_eq!(bale.alignment(), 4096);

        let bale = BaleEocd::new_with_options(512, 256).unwrap();
        assert_eq!(bale.alignment_pow2, 9); // 2^9 = 512
        assert_eq!(bale.alignment(), 512);
    }

    /// Path size is stored correctly.
    #[test]
    fn path_size_encoding() {
        let bale = BaleEocd::new_with_options(4096, 2048).unwrap();
        assert_eq!(bale.path_size(), 2048);

        let bale = BaleEocd::new_with_options(4096, 1).unwrap();
        assert_eq!(bale.path_size(), 1);
    }

    /// Magic signature is correct.
    #[test]
    fn magic_signature() {
        let bale = BaleEocd::new();
        assert_eq!(bale.magic.get(), 0x454C_4142); // "BALE"
        assert!(bale.is_valid());

        // Verify the bytes spell "BALE" in little-endian.
        let bytes = bale.as_bytes();
        assert_eq!(&bytes[0..4], b"BALE");
    }

    /// Version is set to current version.
    #[test]
    fn version() {
        let bale = BaleEocd::new();
        assert_eq!(bale.version(), BaleEocd::CURRENT_VERSION);
    }

    /// Can be serialized and deserialized.
    #[test]
    fn roundtrip() {
        let bale = BaleEocd::new_with_options(8192, 512).unwrap();
        let bytes = bale.as_bytes();
        assert_eq!(bytes.len(), BaleEocd::SIZE);

        let restored = BaleEocd::ref_from_bytes(bytes).unwrap();
        assert!(restored.is_valid());
        assert_eq!(restored.alignment(), 8192);
        assert_eq!(restored.path_size(), 512);
        assert_eq!(restored.version(), BaleEocd::CURRENT_VERSION);
    }

    /// Reserved bytes are all zero.
    #[test]
    fn reserved_is_zero() {
        let bale = BaleEocd::new();
        assert!(bale.reserved.iter().all(|&b| b == 0));
    }

    /// Non-power-of-2 alignment returns an error.
    #[test]
    fn invalid_alignment_returns_error() {
        let result = BaleEocd::new_with_options(1000, 256);
        assert!(matches!(result, Err(BaleError::InvalidAlignment(1000))));
    }

    /// Zero alignment returns an error.
    #[test]
    fn zero_alignment_returns_error() {
        let result = BaleEocd::new_with_options(0, 256);
        assert!(matches!(result, Err(BaleError::InvalidAlignment(0))));
    }

    /// Zero path size returns an error.
    #[test]
    fn zero_path_size_returns_error() {
        let result = BaleEocd::new_with_options(4096, 0);
        assert!(matches!(result, Err(BaleError::InvalidPathSize(0))));
    }

    /// Path size exceeding MAX_PATH_SIZE returns an error.
    #[test]
    fn path_size_too_large_returns_error() {
        let too_large = BaleEocd::MAX_PATH_SIZE + 1;
        let result = BaleEocd::new_with_options(4096, too_large);
        assert!(matches!(result, Err(BaleError::InvalidPathSize(n)) if n == too_large));
    }

    /// Path size at boundaries (MIN_PATH_SIZE and MAX_PATH_SIZE) is valid.
    #[test]
    fn path_size_boundaries_are_valid() {
        assert!(BaleEocd::new_with_options(4096, BaleEocd::MIN_PATH_SIZE).is_ok());
        assert!(BaleEocd::new_with_options(4096, BaleEocd::MAX_PATH_SIZE).is_ok());
    }

    /// Alignment exceeding 16 MB returns an error.
    #[test]
    fn alignment_too_large_returns_error() {
        let too_large = 1u32 << 25; // 32 MB
        let result = BaleEocd::new_with_options(too_large, 256);
        assert!(matches!(result, Err(BaleError::InvalidAlignment(n)) if n == too_large));
    }

    /// Alignment at max (16 MB) is valid.
    #[test]
    fn max_alignment_is_valid() {
        let max_align = 1u32 << BaleEocd::MAX_ALIGNMENT_POW2;
        assert!(BaleEocd::new_with_options(max_align, 256).is_ok());
    }

    // ==================== Malformed Input Tests ====================

    /// Invalid magic signature fails is_valid().
    #[test]
    fn invalid_magic_fails_validation() {
        let mut bale = BaleEocd::new();
        bale.magic = U32::new(0x12345678);
        assert!(!bale.is_valid());
    }

    /// alignment_pow2 just over limit fails is_valid().
    #[test]
    fn alignment_pow2_over_limit_fails_validation() {
        let mut bale = BaleEocd::new();
        bale.alignment_pow2 = BaleEocd::MAX_ALIGNMENT_POW2 + 1; // 25
        assert!(!bale.is_valid());
        // alignment() returns fallback value of 1.
        assert_eq!(bale.alignment(), 1);
    }

    /// alignment_pow2 at max u8 fails is_valid().
    #[test]
    fn alignment_pow2_max_u8_fails_validation() {
        let mut bale = BaleEocd::new();
        bale.alignment_pow2 = 255;
        assert!(!bale.is_valid());
        // alignment() returns fallback value of 1.
        assert_eq!(bale.alignment(), 1);
    }

    /// path_size = 0 fails is_valid().
    #[test]
    fn path_size_zero_fails_validation() {
        let mut bale = BaleEocd::new();
        bale.path_size = U16::new(0);
        assert!(!bale.is_valid());
    }

    /// path_size above max fails is_valid().
    #[test]
    fn path_size_over_max_fails_validation() {
        let mut bale = BaleEocd::new();
        bale.path_size = U16::new(3000);
        assert!(!bale.is_valid());
    }

    /// Non-zero reserved bytes with valid fields still passes is_valid().
    #[test]
    fn nonzero_reserved_passes_validation() {
        let mut bale = BaleEocd::new();
        bale.reserved[0] = 0xFF;
        bale.reserved[100] = 0xAB;
        // is_valid() ignores reserved field for forward compatibility.
        assert!(bale.is_valid());
    }
}
