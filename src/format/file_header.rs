//! File header at offset 0 of every bale archive.

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

/// File header (8 bytes) identifying the archive and format version.
///
/// Located at byte offset 0 of every bale archive. The magic bytes allow
/// `file(1)` and similar tools to identify bale archives by reading the
/// first 5 bytes. The version bytes enable readers to select the appropriate
/// parser for the format version.
///
/// # Layout
///
/// | Offset | Size | Field         | Description                            |
/// |--------|------|---------------|----------------------------------------|
/// | 0      | 5    | Magic         | `"BALE\0"` (0x42 0x41 0x4C 0x45 0x00) |
/// | 5      | 1    | Major version | Format major version                   |
/// | 6      | 1    | Minor version | Format minor version                   |
/// | 7      | 1    | Patch version | Format patch version                   |
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
pub struct FileHeader {
    /// Magic bytes: `"BALE\0"` (0x42 0x41 0x4C 0x45 0x00).
    pub magic: [u8; Self::MAGIC_SIZE],
    /// Format major version.
    pub version_major: u8,
    /// Format minor version.
    pub version_minor: u8,
    /// Format patch version.
    pub version_patch: u8,
}

impl FileHeader {
    /// Total size of the file header in bytes.
    pub const SIZE: usize = 8;

    /// Size of the magic field in bytes.
    pub const MAGIC_SIZE: usize = 5;

    /// Expected magic bytes: `"BALE\0"`.
    pub const MAGIC: [u8; Self::MAGIC_SIZE] = *b"BALE\0";

    /// Current format version (1.0.0).
    pub const CURRENT_VERSION: (u8, u8, u8) = (1, 0, 0);

    /// Creates a new `FileHeader` with the current format version.
    #[must_use]
    pub fn new() -> Self {
        let (major, minor, patch) = Self::CURRENT_VERSION;
        Self {
            magic: Self::MAGIC,
            version_major: major,
            version_minor: minor,
            version_patch: patch,
        }
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
}

impl Default for FileHeader {
    /// Returns a `FileHeader` with the current format version.
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Structure must be exactly 8 bytes.
    #[test]
    fn size_is_8_bytes() {
        assert_eq!(std::mem::size_of::<FileHeader>(), FileHeader::SIZE);
        assert_eq!(FileHeader::SIZE, 8);
    }

    /// Default header has correct magic and version.
    #[test]
    fn default_header() {
        let header = FileHeader::new();
        assert!(header.has_valid_magic());
        assert_eq!(header.version(), FileHeader::CURRENT_VERSION);
    }

    /// Magic bytes spell "BALE\0".
    #[test]
    fn magic_bytes() {
        let header = FileHeader::new();
        assert_eq!(&header.magic, b"BALE\0");
    }

    /// Invalid magic is detected.
    #[test]
    fn invalid_magic() {
        let mut header = FileHeader::new();
        header.magic[0] = b'X';
        assert!(!header.has_valid_magic());
    }

    /// Round-trip serialization preserves all fields.
    #[test]
    fn roundtrip() {
        let header = FileHeader::new();
        let bytes = header.as_bytes();
        assert_eq!(bytes.len(), FileHeader::SIZE);

        let restored = FileHeader::ref_from_bytes(bytes).unwrap();
        assert!(restored.has_valid_magic());
        assert_eq!(restored.version(), FileHeader::CURRENT_VERSION);
    }

    /// Raw byte layout matches spec.
    #[test]
    fn byte_layout() {
        let header = FileHeader::new();
        let bytes = header.as_bytes();
        // Magic: "BALE\0"
        assert_eq!(&bytes[0..5], b"BALE\0");
        // Version: 1.0.0
        assert_eq!(bytes[5], 1); // major
        assert_eq!(bytes[6], 0); // minor
        assert_eq!(bytes[7], 0); // patch
    }

    // ==================== Property Tests ====================

    use crate::proptest_config;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(proptest_config::config())]

        /// Arbitrary 8-byte input never panics when interpreted as a FileHeader.
        ///
        /// Exercises `ref_from_bytes`, `has_valid_magic`, and `version` on
        /// random byte patterns.
        #[test]
        fn fuzz_file_header_parsing(data in prop::collection::vec(any::<u8>(), 8..=8)) {
            let header = FileHeader::ref_from_bytes(&data).unwrap();
            let _ = header.has_valid_magic();
            let _ = header.version();
        }
    }
}
