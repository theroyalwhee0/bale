//! Archive trailer at the end of every bale archive.

use super::Crc;
use crate::BaleError;
use bitflags::bitflags;
use zerocopy::byteorder::little_endian::{U16, U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

bitflags! {
    /// Trailer flags bitfield.
    ///
    /// Stored as a `u8` in the trailer at offset 39.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct TrailerFlags: u8 {
        /// Directory table is fully sorted with no tombstones.
        ///
        /// When set, readers may use binary search for path lookup.
        /// When clear, readers must use linear scan.
        const COMPACTED = 0x01;
    }
}

/// Archive trailer (64 bytes), always the last 64 bytes of the file.
///
/// Contains all archive configuration, table offsets, and counts needed to
/// read the archive. The trailer is the entry point for reading: a reader
/// reads the last 64 bytes, validates the magic, and uses the offsets to
/// locate the entry and directory tables.
///
/// # Layout
///
/// | Offset | Size | Field                  | Description                              |
/// |--------|------|------------------------|------------------------------------------|
/// | 0      | 8    | Entry table offset     | LE, byte offset to entry table           |
/// | 8      | 8    | Directory table offset | LE, byte offset to directory table       |
/// | 16     | 8    | Archive size           | LE, expected total file size in bytes    |
/// | 24     | 4    | Entry count            | LE, number of entry rows                 |
/// | 28     | 4    | Directory entry count  | LE, number of directory rows             |
/// | 32     | 4    | Next entry ID          | LE, next ID to assign                    |
/// | 36     | 2    | Path size              | LE, maximum path length (1-4096)         |
/// | 38     | 1    | Alignment power        | Exponent N where alignment = 2^N (0-16)  |
/// | 39     | 1    | Flags                  | Bitfield (see [`TrailerFlags`])           |
/// | 40     | 16   | Reserved               | Must be zero                             |
/// | 56     | 4    | Magic                  | `BALE` (0x42 0x41 0x4C 0x45)             |
/// | 60     | 4    | Metadata CRC-32C       | CRC-32C over header + tables + trailer   |
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub struct Trailer {
    /// Byte offset to the entry table (0 = no table).
    pub entry_table_offset: U64,
    /// Byte offset to the directory table (0 = no table).
    pub directory_table_offset: U64,
    /// Expected total archive file size in bytes.
    pub archive_size: U64,
    /// Number of entry rows (including tombstones).
    pub entry_count: U32,
    /// Number of directory rows (including tombstones).
    pub directory_entry_count: U32,
    /// Next entry ID to assign.
    pub next_id: U32,
    /// Maximum path length in bytes (1-4096).
    pub path_size: U16,
    /// Alignment as power of 2 (e.g., 12 means 2^12 = 4096 bytes).
    pub alignment_power: u8,
    /// Flags bitfield (see [`TrailerFlags`]).
    pub flags: u8,
    /// Reserved for future use. Must be zero.
    pub reserved: [u8; Self::RESERVED_SIZE],
    /// Magic bytes: `BALE` (0x42 0x41 0x4C 0x45).
    pub magic: [u8; Self::MAGIC_SIZE],
    /// Metadata CRC-32C over file header + entry table + directory table +
    /// trailer bytes 0-59.
    pub metadata_crc32c: U32,
}

impl Trailer {
    /// Total size of the trailer in bytes.
    pub const SIZE: usize = 64;

    /// Size of the magic field in bytes (`BALE`, no null terminator).
    pub const MAGIC_SIZE: usize = 4;

    /// Size of the reserved field in bytes.
    const RESERVED_SIZE: usize = 16;

    /// Expected magic bytes: `BALE`.
    pub const MAGIC: [u8; Self::MAGIC_SIZE] = *b"BALE";

    /// Minimum allowed path size.
    pub const MIN_PATH_SIZE: u16 = 1;

    /// Maximum allowed path size.
    pub const MAX_PATH_SIZE: u16 = 4096;

    /// Maximum alignment power (2^16 = 64 KB).
    pub const MAX_ALIGNMENT_POWER: u8 = 16;

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
        Self {
            entry_table_offset: U64::new(0),
            directory_table_offset: U64::new(0),
            archive_size: U64::new(0),
            entry_count: U32::new(0),
            directory_entry_count: U32::new(0),
            next_id: U32::new(1),
            path_size: U16::new(Self::DEFAULT_PATH_SIZE),
            alignment_power: Self::DEFAULT_ALIGNMENT_POWER,
            flags: TrailerFlags::COMPACTED.bits(),
            reserved: [0u8; Self::RESERVED_SIZE],
            magic: Self::MAGIC,
            metadata_crc32c: U32::new(0),
        }
    }

    /// Creates a new `Trailer` with the given parameters.
    ///
    /// # Arguments
    ///
    /// * `alignment` - Alignment in bytes (must be a power of 2, max 2^16)
    /// * `path_size` - Maximum path size (1..=4096)
    ///
    /// # Errors
    ///
    /// - Returns `BaleError::InvalidAlignment` if `alignment` is not a power
    ///   of 2, is zero, or exceeds 64 KB.
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
        Ok(Self {
            entry_table_offset: U64::new(0),
            directory_table_offset: U64::new(0),
            archive_size: U64::new(0),
            entry_count: U32::new(0),
            directory_entry_count: U32::new(0),
            next_id: U32::new(1),
            path_size: U16::new(path_size),
            alignment_power,
            flags: TrailerFlags::COMPACTED.bits(),
            reserved: [0u8; Self::RESERVED_SIZE],
            magic: Self::MAGIC,
            metadata_crc32c: U32::new(0),
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

    /// Returns the expected total archive file size.
    #[must_use]
    pub const fn archive_size(&self) -> u64 {
        self.archive_size.get()
    }

    /// Sets the expected total archive file size.
    pub fn set_archive_size(&mut self, size: u64) {
        self.archive_size = U64::new(size);
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

    /// Returns the trailer flags as a [`TrailerFlags`] bitfield.
    #[must_use]
    pub fn flags(&self) -> TrailerFlags {
        TrailerFlags::from_bits_truncate(self.flags)
    }

    /// Returns `true` if the compacted flag is set.
    ///
    /// When compacted, the directory table is fully sorted with no tombstones,
    /// enabling binary search for path lookups.
    #[must_use]
    pub fn is_compacted(&self) -> bool {
        self.flags().contains(TrailerFlags::COMPACTED)
    }

    /// Sets the compacted flag.
    ///
    /// Call after sorting the directory table and removing all tombstones.
    pub fn set_compacted(&mut self) {
        self.flags = (self.flags() | TrailerFlags::COMPACTED).bits();
    }

    /// Clears the compacted flag.
    ///
    /// Call after any mutation that may leave the directory table unsorted
    /// or containing tombstones (e.g., insertion or deletion without full
    /// re-sort).
    pub fn clear_compacted(&mut self) {
        self.flags = (self.flags() - TrailerFlags::COMPACTED).bits();
    }

    /// Returns `true` if the magic bytes match the expected value.
    #[must_use]
    pub const fn has_valid_magic(&self) -> bool {
        let m = &self.magic;
        m[0] == Self::MAGIC[0]
            && m[1] == Self::MAGIC[1]
            && m[2] == Self::MAGIC[2]
            && m[3] == Self::MAGIC[3]
    }

    /// Validates the trailer fields.
    ///
    /// Checks:
    /// - Magic bytes are `BALE` at offset 56
    /// - `alignment_power` is within valid range (≤ 16)
    /// - `path_size` is in range 1..=4096
    /// - `next_id` is non-zero (entry IDs start at 1)
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
            && self.next_id.get() > 0
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

    /// Computes the metadata CRC-32C over the given archive regions.
    ///
    /// The CRC is computed over the logical concatenation of four
    /// non-contiguous regions, fed sequentially into the CRC state machine:
    ///
    /// 1. The file header (8 bytes at offset 0)
    /// 2. The entire entry table (all bytes)
    /// 3. The entire directory table (all bytes)
    /// 4. Trailer bytes 0–59 (all fields except the CRC itself)
    ///
    /// During computation, the CRC field (trailer bytes 60–63) is treated as
    /// zero. Empty tables contribute zero bytes.
    #[must_use]
    pub fn compute_metadata_crc(
        file_header: &[u8],
        entry_table: &[u8],
        directory_table: &[u8],
        trailer: &Self,
    ) -> Crc {
        let trailer_bytes = trailer.as_bytes();
        Crc::NONE
            .append(file_header)
            .append(entry_table)
            .append(directory_table)
            .append(&trailer_bytes[..60])
    }

    /// Sets the metadata CRC-32C field.
    pub fn set_metadata_crc(&mut self, crc: Crc) {
        self.metadata_crc32c = U32::new(crc.to_u32());
    }

    /// Returns the stored metadata CRC-32C value.
    #[must_use]
    pub fn metadata_crc(&self) -> Crc {
        Crc::new(self.metadata_crc32c.get())
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
        assert_eq!(trailer.archive_size(), 0);
        assert_eq!(trailer.metadata_crc32c.get(), 0);
        assert!(trailer.is_valid());
    }

    /// Magic bytes spell "BALE" at offset 56.
    #[test]
    fn magic_bytes() {
        let trailer = Trailer::new();
        assert_eq!(&trailer.magic, b"BALE");
        assert!(trailer.has_valid_magic());
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

    /// New trailers have the compacted flag set (empty = trivially compacted).
    #[test]
    fn new_trailer_is_compacted() {
        let trailer = Trailer::new();
        assert!(trailer.is_compacted());
        assert!(trailer.flags().contains(TrailerFlags::COMPACTED));
    }

    /// new_with_options also sets the compacted flag.
    #[test]
    fn new_with_options_is_compacted() {
        let trailer = Trailer::new_with_options(4096, 256).unwrap();
        assert!(trailer.is_compacted());
    }

    /// set_compacted and clear_compacted toggle the flag.
    #[test]
    fn compacted_flag_toggle() {
        let mut trailer = Trailer::new();
        assert!(trailer.is_compacted());

        trailer.clear_compacted();
        assert!(!trailer.is_compacted());
        assert_eq!(trailer.flags, 0);

        trailer.set_compacted();
        assert!(trailer.is_compacted());
        assert_eq!(trailer.flags, TrailerFlags::COMPACTED.bits());
    }

    /// Compacted flag survives round-trip serialization.
    #[test]
    fn compacted_flag_roundtrip() {
        let mut trailer = Trailer::new();
        trailer.clear_compacted();

        let bytes = trailer.as_bytes();
        let restored = Trailer::ref_from_bytes(bytes).unwrap();
        assert!(!restored.is_compacted());

        let trailer2 = Trailer::new();
        let bytes2 = trailer2.as_bytes();
        let restored2 = Trailer::ref_from_bytes(bytes2).unwrap();
        assert!(restored2.is_compacted());
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
        trailer.set_archive_size(9999);

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
        assert_eq!(restored.archive_size(), 9999);
    }

    /// archive_size can be set and retrieved.
    #[test]
    fn archive_size_accessors() {
        let mut trailer = Trailer::new();
        assert_eq!(trailer.archive_size(), 0);

        trailer.set_archive_size(12345);
        assert_eq!(trailer.archive_size(), 12345);
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

    /// Alignment exceeding 64 KB returns an error.
    #[test]
    fn alignment_too_large_returns_error() {
        let too_large = 1u32 << 17;
        let result = Trailer::new_with_options(too_large, 256);
        assert!(matches!(result, Err(BaleError::InvalidAlignment(ref s)) if s.contains("exceeds")));
    }

    /// Alignment at max (64 KB) is valid.
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

    /// next_id = 0 fails is_valid().
    #[test]
    fn next_id_zero_fails_validation() {
        let mut trailer = Trailer::new();
        trailer.set_next_id(0);
        assert!(!trailer.is_valid());
    }

    /// next_id = u32::MAX passes is_valid() (archive is full but valid).
    #[test]
    fn next_id_max_passes_validation() {
        let mut trailer = Trailer::new();
        trailer.set_next_id(u32::MAX);
        assert!(trailer.is_valid());
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

    // ==================== Property Tests ====================

    use crate::proptest_config;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(proptest_config::config())]

        /// Arbitrary 64-byte input never panics when interpreted as a Trailer.
        ///
        /// Exercises `ref_from_bytes`, `is_valid`, `validated`, `has_valid_magic`,
        /// and `alignment` on random byte patterns.
        #[test]
        fn fuzz_trailer_parsing(data in prop::collection::vec(any::<u8>(), 64..=64)) {
            let trailer = Trailer::ref_from_bytes(&data).unwrap();
            let _ = trailer.is_valid();
            let _ = trailer.validated();
            let _ = trailer.has_valid_magic();
            let _ = trailer.alignment();
            let _ = trailer.path_size();
            let _ = trailer.archive_size();
            let _ = trailer.next_id();
            let _ = trailer.flags();
            let _ = trailer.is_compacted();
            let _ = trailer.metadata_crc();
        }
    }

    /// Byte layout matches spec offsets.
    #[test]
    fn byte_layout_matches_spec() {
        let mut trailer = Trailer::new();
        trailer.entry_table_offset = U64::new(0x1122_3344_5566_7788);
        trailer.directory_table_offset = U64::new(0xAABB_CCDD_EEFF_0011);
        trailer.set_archive_size(0x0102_0304_0506_0708);

        let bytes = trailer.as_bytes();

        // entry_table_offset at offset 0 (8 bytes, LE).
        assert_eq!(&bytes[0..8], 0x1122_3344_5566_7788_u64.to_le_bytes());
        // directory_table_offset at offset 8 (8 bytes, LE).
        assert_eq!(&bytes[8..16], 0xAABB_CCDD_EEFF_0011_u64.to_le_bytes());
        // archive_size at offset 16 (8 bytes, LE).
        assert_eq!(&bytes[16..24], 0x0102_0304_0506_0708_u64.to_le_bytes());
        // path_size at offset 36 (2 bytes, LE).
        assert_eq!(&bytes[36..38], 256_u16.to_le_bytes());
        // alignment_power at offset 38 (1 byte).
        assert_eq!(bytes[38], 12);
        // flags at offset 39 (1 byte).
        assert_eq!(bytes[39], TrailerFlags::COMPACTED.bits());
        // magic at offset 56 (4 bytes).
        assert_eq!(&bytes[56..60], b"BALE");
        // metadata_crc32c at offset 60 (4 bytes).
        assert_eq!(&bytes[60..64], &[0, 0, 0, 0]);
    }

    /// Metadata CRC is deterministic and non-zero for non-empty inputs.
    #[test]
    fn metadata_crc_deterministic() {
        let header = [0x42, 0x41, 0x4C, 0x45, 0x00, 1, 0, 0]; // BALE\0 v1.0.0
        let trailer = Trailer::new();

        let crc1 = Trailer::compute_metadata_crc(&header, &[], &[], &trailer);
        let crc2 = Trailer::compute_metadata_crc(&header, &[], &[], &trailer);
        assert_eq!(crc1, crc2);
        // Non-zero since input data is non-empty.
        assert_ne!(crc1, Crc::NONE);
    }

    /// Metadata CRC changes when trailer fields change.
    #[test]
    fn metadata_crc_changes_with_trailer() {
        let header = [0x42, 0x41, 0x4C, 0x45, 0x00, 1, 0, 0];
        let trailer1 = Trailer::new();

        let mut trailer2 = Trailer::new();
        trailer2.set_archive_size(9999);

        let crc1 = Trailer::compute_metadata_crc(&header, &[], &[], &trailer1);
        let crc2 = Trailer::compute_metadata_crc(&header, &[], &[], &trailer2);
        assert_ne!(crc1, crc2);
    }

    /// set_metadata_crc and metadata_crc round-trip.
    #[test]
    fn metadata_crc_accessors() {
        let mut trailer = Trailer::new();
        assert_eq!(trailer.metadata_crc(), Crc::NONE);

        let crc = Crc::new(0xDEAD_BEEF);
        trailer.set_metadata_crc(crc);
        assert_eq!(trailer.metadata_crc(), crc);
    }
}
