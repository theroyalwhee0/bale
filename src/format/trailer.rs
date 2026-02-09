//! Archive trailer at the end of every bale archive.

use crate::BaleError;
use zerocopy::byteorder::little_endian::{U16, U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

/// Archive trailer (64 bytes), always the last 64 bytes of the file.
///
/// Contains all archive configuration, table offsets, and counts needed to
/// read the archive. The trailer is the entry point for reading: a reader
/// reads the last 64 bytes, validates the magic, and uses the offsets to
/// locate the entry and directory tables.
///
/// # Layout
///
/// | Offset | Size | Field                | Description                        |
/// |--------|------|----------------------|------------------------------------|
/// | 0      | 5    | Magic                | `"BALE\0"`                         |
/// | 5      | 1    | Major version        | Format major version               |
/// | 6      | 1    | Minor version        | Format minor version               |
/// | 7      | 1    | Patch version        | Format patch version               |
/// | 8      | 8    | Entry table offset   | LE, byte offset to entry table     |
/// | 16     | 4    | Entry count          | LE, number of entry rows           |
/// | 20     | 8    | Directory table offset | LE, byte offset to directory table |
/// | 28     | 4    | Directory entry count | LE, number of directory rows       |
/// | 32     | 4    | Next entry ID        | LE, next ID to assign              |
/// | 36     | 1    | Alignment power      | Exponent N where alignment = 2^N   |
/// | 37     | 2    | Path size            | LE, maximum path length (1-4096)   |
/// | 39     | 25   | Reserved             | Must be zero                       |
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub struct Trailer {
    /// Magic bytes: `"BALE\0"` (0x42 0x41 0x4C 0x45 0x00).
    pub magic: [u8; Self::MAGIC_SIZE],
    /// Format major version.
    pub version_major: u8,
    /// Format minor version.
    pub version_minor: u8,
    /// Format patch version.
    pub version_patch: u8,
    /// Byte offset to the entry table.
    pub entry_table_offset: U64,
    /// Number of entry rows.
    pub entry_count: U32,
    /// Byte offset to the directory table.
    pub directory_table_offset: U64,
    /// Number of directory rows.
    pub directory_entry_count: U32,
    /// Next entry ID to assign.
    pub next_id: U32,
    /// Alignment as power of 2 (e.g., 12 means 2^12 = 4096 bytes).
    pub alignment_power: u8,
    /// Maximum path length in bytes (1-4096).
    pub path_size: U16,
    /// Reserved for future use. Must be zero.
    pub reserved: [u8; Self::RESERVED_SIZE],
}

impl Trailer {
    /// Total size of the trailer in bytes.
    pub const SIZE: usize = 64;

    /// Size of the magic field in bytes.
    pub const MAGIC_SIZE: usize = 5;

    /// Size of the reserved field in bytes.
    const RESERVED_SIZE: usize = 25;

    /// Expected magic bytes: `"BALE\0"`.
    pub const MAGIC: [u8; Self::MAGIC_SIZE] = *b"BALE\0";

    /// Current format version (1.0.0).
    pub const CURRENT_VERSION: (u8, u8, u8) = (1, 0, 0);

    /// Minimum allowed path size.
    pub const MIN_PATH_SIZE: u16 = 1;

    /// Maximum allowed path size.
    pub const MAX_PATH_SIZE: u16 = 4096;

    /// Maximum alignment power (2^24 = 16 MB).
    pub const MAX_ALIGNMENT_POWER: u8 = 24;

    /// Default alignment power (2^12 = 4096 bytes).
    pub const DEFAULT_ALIGNMENT_POWER: u8 = 12;

    /// Default alignment in bytes (4096 = page size).
    pub const DEFAULT_ALIGNMENT: u32 = 1 << Self::DEFAULT_ALIGNMENT_POWER;

    /// Default path size in bytes.
    pub const DEFAULT_PATH_SIZE: u16 = 256;

    /// Minimum archive size (file header + trailer).
    pub const MIN_ARCHIVE_SIZE: u64 = 8 + Self::SIZE as u64;

    /// Creates a new `Trailer` with default settings and empty tables.
    #[must_use]
    pub fn new() -> Self {
        let (major, minor, patch) = Self::CURRENT_VERSION;
        Self {
            magic: Self::MAGIC,
            version_major: major,
            version_minor: minor,
            version_patch: patch,
            entry_table_offset: U64::new(0),
            entry_count: U32::new(0),
            directory_table_offset: U64::new(0),
            directory_entry_count: U32::new(0),
            next_id: U32::new(1),
            alignment_power: Self::DEFAULT_ALIGNMENT_POWER,
            path_size: U16::new(Self::DEFAULT_PATH_SIZE),
            reserved: [0u8; Self::RESERVED_SIZE],
        }
    }

    /// Creates a new `Trailer` with the given parameters.
    ///
    /// # Arguments
    ///
    /// * `alignment` - Alignment in bytes (must be a power of 2, max 2^24)
    /// * `path_size` - Maximum path size (1..=4096)
    ///
    /// # Errors
    ///
    /// - Returns `BaleError::InvalidAlignment` if `alignment` is not a power
    ///   of 2, is zero, or exceeds 16 MB.
    /// - Returns `BaleError::InvalidPathSize` if `path_size` is not in range
    ///   1..=4096.
    pub fn new_with_options(alignment: u32, path_size: u16) -> Result<Self, BaleError> {
        let max_alignment = 1u32 << Self::MAX_ALIGNMENT_POWER;
        if alignment == 0 {
            return Err(BaleError::InvalidAlignment("0 is not a power of 2".into()));
        }
        if !alignment.is_power_of_two() {
            return Err(BaleError::InvalidAlignment(format!(
                "{alignment} is not a power of 2"
            )));
        }
        if alignment > max_alignment {
            return Err(BaleError::InvalidAlignment(format!(
                "{alignment} exceeds maximum of {max_alignment}"
            )));
        }
        if !(Self::MIN_PATH_SIZE..=Self::MAX_PATH_SIZE).contains(&path_size) {
            return Err(BaleError::InvalidPathSize(path_size));
        }
        let alignment_power = alignment.trailing_zeros() as u8;
        let (major, minor, patch) = Self::CURRENT_VERSION;
        Ok(Self {
            magic: Self::MAGIC,
            version_major: major,
            version_minor: minor,
            version_patch: patch,
            entry_table_offset: U64::new(0),
            entry_count: U32::new(0),
            directory_table_offset: U64::new(0),
            directory_entry_count: U32::new(0),
            next_id: U32::new(1),
            alignment_power,
            path_size: U16::new(path_size),
            reserved: [0u8; Self::RESERVED_SIZE],
        })
    }

    /// Returns the alignment in bytes.
    ///
    /// # Errors
    ///
    /// Returns `BaleError::InvalidAlignment` if `alignment_power` exceeds
    /// [`MAX_ALIGNMENT_POWER`](Self::MAX_ALIGNMENT_POWER).
    pub fn alignment(&self) -> Result<u32, BaleError> {
        if self.alignment_power > Self::MAX_ALIGNMENT_POWER {
            return Err(BaleError::InvalidAlignment(format!(
                "2^{} exceeds maximum of 2^{}",
                self.alignment_power,
                Self::MAX_ALIGNMENT_POWER
            )));
        }
        Ok(1 << self.alignment_power)
    }

    /// Returns the maximum path size.
    #[must_use]
    pub const fn path_size(&self) -> u16 {
        self.path_size.get()
    }

    /// Returns the next entry ID to assign.
    #[must_use]
    pub const fn next_id(&self) -> u32 {
        self.next_id.get()
    }

    /// Sets the next entry ID to assign.
    pub fn set_next_id(&mut self, id: u32) {
        self.next_id = U32::new(id);
    }

    /// Returns the version as a tuple (major, minor, patch).
    #[must_use]
    pub const fn version(&self) -> (u8, u8, u8) {
        (self.version_major, self.version_minor, self.version_patch)
    }

    /// Returns `true` if the magic bytes match the expected value.
    #[must_use]
    pub const fn has_valid_magic(&self) -> bool {
        let m = &self.magic;
        m[0] == Self::MAGIC[0]
            && m[1] == Self::MAGIC[1]
            && m[2] == Self::MAGIC[2]
            && m[3] == Self::MAGIC[3]
            && m[4] == Self::MAGIC[4]
    }

    /// Validates the trailer fields.
    ///
    /// Checks:
    /// - Magic bytes are `"BALE\0"`
    /// - `alignment_power` is within valid range (≤ 24)
    /// - `path_size` is in range 1..=4096
    ///
    /// Note: The `reserved` field is NOT checked. Non-zero reserved bytes are
    /// silently ignored for forward compatibility with future format extensions.
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        let path_size = self.path_size.get();
        self.has_valid_magic()
            && self.alignment_power <= Self::MAX_ALIGNMENT_POWER
            && path_size >= Self::MIN_PATH_SIZE
            && path_size <= Self::MAX_PATH_SIZE
    }

    /// Validates the trailer and returns a reference or an error.
    ///
    /// # Errors
    ///
    /// Returns `BaleError::Corrupted` if any validation check fails.
    pub fn validated(&self) -> Result<&Self, BaleError> {
        if self.is_valid() {
            Ok(self)
        } else {
            Err(BaleError::Corrupted("invalid trailer".into()))
        }
    }
}

impl Default for Trailer {
    /// Returns a `Trailer` with default settings.
    ///
    /// See [`Trailer::new()`] for details.
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Structure must be exactly 64 bytes.
    #[test]
    fn size_is_64_bytes() {
        assert_eq!(std::mem::size_of::<Trailer>(), Trailer::SIZE);
        assert_eq!(Trailer::SIZE, 64);
    }

    /// Minimum archive size is 72 bytes (8 + 64).
    #[test]
    fn min_archive_size() {
        assert_eq!(Trailer::MIN_ARCHIVE_SIZE, 72);
    }

    /// Default settings use DEFAULT_ALIGNMENT and DEFAULT_PATH_SIZE.
    #[test]
    fn default_settings() {
        let trailer = Trailer::new();
        assert_eq!(trailer.alignment().unwrap(), Trailer::DEFAULT_ALIGNMENT);
        assert_eq!(trailer.path_size(), Trailer::DEFAULT_PATH_SIZE);
        assert_eq!(trailer.next_id(), 1);
        assert_eq!(trailer.entry_count.get(), 0);
        assert_eq!(trailer.directory_entry_count.get(), 0);
        assert!(trailer.is_valid());
    }

    /// Magic bytes spell "BALE\0".
    #[test]
    fn magic_bytes() {
        let trailer = Trailer::new();
        assert_eq!(&trailer.magic, b"BALE\0");
        assert!(trailer.has_valid_magic());
    }

    /// Version is set to current version.
    #[test]
    fn version() {
        let trailer = Trailer::new();
        assert_eq!(trailer.version(), Trailer::CURRENT_VERSION);
    }

    /// Alignment is correctly encoded as power of 2.
    #[test]
    fn alignment_encoding() {
        let trailer =
            Trailer::new_with_options(Trailer::DEFAULT_ALIGNMENT, Trailer::DEFAULT_PATH_SIZE)
                .unwrap();
        assert_eq!(trailer.alignment_power, Trailer::DEFAULT_ALIGNMENT_POWER);
        assert_eq!(trailer.alignment().unwrap(), Trailer::DEFAULT_ALIGNMENT);

        let trailer = Trailer::new_with_options(512, 256).unwrap();
        assert_eq!(trailer.alignment_power, 9); // 2^9 = 512
        assert_eq!(trailer.alignment().unwrap(), 512);
    }

    /// Minimum alignment (1 byte, power = 0) is valid.
    #[test]
    fn min_alignment_is_valid() {
        let trailer = Trailer::new_with_options(1, 256).unwrap();
        assert_eq!(trailer.alignment_power, 0);
        assert_eq!(trailer.alignment().unwrap(), 1);
        assert!(trailer.is_valid());
    }

    /// Path size is stored correctly.
    #[test]
    fn path_size_encoding() {
        let trailer = Trailer::new_with_options(4096, 4096).unwrap();
        assert_eq!(trailer.path_size(), 4096);

        let trailer = Trailer::new_with_options(4096, 1).unwrap();
        assert_eq!(trailer.path_size(), 1);
    }

    /// Next ID starts at 1 and can be modified.
    #[test]
    fn next_id() {
        let mut trailer = Trailer::new();
        assert_eq!(trailer.next_id(), 1);

        trailer.set_next_id(42);
        assert_eq!(trailer.next_id(), 42);
    }

    /// Reserved bytes are all zero.
    #[test]
    fn reserved_is_zero() {
        let trailer = Trailer::new();
        assert!(trailer.reserved.iter().all(|&b| b == 0));
    }

    /// Round-trip serialization preserves all fields.
    #[test]
    fn roundtrip() {
        let mut trailer = Trailer::new_with_options(8192, 512).unwrap();
        trailer.entry_table_offset = U64::new(4096);
        trailer.entry_count = U32::new(10);
        trailer.directory_table_offset = U64::new(4576);
        trailer.directory_entry_count = U32::new(15);
        trailer.set_next_id(11);

        let bytes = trailer.as_bytes();
        assert_eq!(bytes.len(), Trailer::SIZE);

        let restored = Trailer::ref_from_bytes(bytes).unwrap();
        assert!(restored.is_valid());
        assert_eq!(restored.alignment().unwrap(), 8192);
        assert_eq!(restored.path_size(), 512);
        assert_eq!(restored.entry_table_offset.get(), 4096);
        assert_eq!(restored.entry_count.get(), 10);
        assert_eq!(restored.directory_table_offset.get(), 4576);
        assert_eq!(restored.directory_entry_count.get(), 15);
        assert_eq!(restored.next_id(), 11);
        assert_eq!(restored.version(), Trailer::CURRENT_VERSION);
    }

    /// Non-power-of-2 alignment returns an error.
    #[test]
    fn invalid_alignment_returns_error() {
        let result = Trailer::new_with_options(1000, 256);
        assert!(matches!(result, Err(BaleError::InvalidAlignment(ref s)) if s.contains("1000")));
    }

    /// Zero alignment returns an error.
    #[test]
    fn zero_alignment_returns_error() {
        let result = Trailer::new_with_options(0, 256);
        assert!(matches!(result, Err(BaleError::InvalidAlignment(ref s)) if s.contains("0")));
    }

    /// Zero path size returns an error.
    #[test]
    fn zero_path_size_returns_error() {
        let result = Trailer::new_with_options(4096, 0);
        assert!(matches!(result, Err(BaleError::InvalidPathSize(0))));
    }

    /// Path size exceeding MAX_PATH_SIZE returns an error.
    #[test]
    fn path_size_too_large_returns_error() {
        let too_large = Trailer::MAX_PATH_SIZE + 1;
        let result = Trailer::new_with_options(4096, too_large);
        assert!(matches!(result, Err(BaleError::InvalidPathSize(n)) if n == too_large));
    }

    /// Path size at boundaries is valid.
    #[test]
    fn path_size_boundaries_are_valid() {
        assert!(Trailer::new_with_options(4096, Trailer::MIN_PATH_SIZE).is_ok());
        assert!(Trailer::new_with_options(4096, Trailer::MAX_PATH_SIZE).is_ok());
    }

    /// Alignment exceeding 16 MB returns an error.
    #[test]
    fn alignment_too_large_returns_error() {
        let too_large = 1u32 << 25;
        let result = Trailer::new_with_options(too_large, 256);
        assert!(matches!(result, Err(BaleError::InvalidAlignment(ref s)) if s.contains("exceeds")));
    }

    /// Alignment at max (16 MB) is valid.
    #[test]
    fn max_alignment_is_valid() {
        let max_align = 1u32 << Trailer::MAX_ALIGNMENT_POWER;
        assert!(Trailer::new_with_options(max_align, 256).is_ok());
    }

    /// Invalid magic fails is_valid().
    #[test]
    fn invalid_magic_fails_validation() {
        let mut trailer = Trailer::new();
        trailer.magic[0] = b'X';
        assert!(!trailer.is_valid());
        assert!(!trailer.has_valid_magic());
    }

    /// alignment_power over limit fails is_valid().
    #[test]
    fn alignment_power_over_limit_fails_validation() {
        let mut trailer = Trailer::new();
        trailer.alignment_power = Trailer::MAX_ALIGNMENT_POWER + 1;
        assert!(!trailer.is_valid());
        assert!(trailer.alignment().is_err());
    }

    /// path_size = 0 fails is_valid().
    #[test]
    fn path_size_zero_fails_validation() {
        let mut trailer = Trailer::new();
        trailer.path_size = U16::new(0);
        assert!(!trailer.is_valid());
    }

    /// path_size above max fails is_valid().
    #[test]
    fn path_size_over_max_fails_validation() {
        let mut trailer = Trailer::new();
        trailer.path_size = U16::new(5000);
        assert!(!trailer.is_valid());
    }

    /// Non-zero reserved bytes still pass is_valid().
    #[test]
    fn nonzero_reserved_passes_validation() {
        let mut trailer = Trailer::new();
        trailer.reserved[0] = 0xFF;
        trailer.reserved[10] = 0xAB;
        assert!(trailer.is_valid());
    }

    /// validated() returns Ok for valid trailers.
    #[test]
    fn validated_returns_ok_for_valid() {
        let trailer = Trailer::new();
        assert!(trailer.validated().is_ok());
    }

    /// validated() returns Err for invalid trailers.
    #[test]
    fn validated_returns_err_for_invalid() {
        let mut trailer = Trailer::new();
        trailer.magic[0] = b'X';
        assert!(trailer.validated().is_err());
    }
}
