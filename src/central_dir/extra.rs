//! Bale extra field for storing entry IDs in ZIP-compatible format.

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, U16, U32};

/// Bale extra field tag (0xBA1D = "BAID" with 1 as I).
const BALE_EXTRA_TAG: u16 = 0xBA1D;

/// Size of the Bale extra field data (u32 ID).
const BALE_EXTRA_DATA_SIZE: u16 = 4;

/// Bale extra field stored in ZIP central directory entries.
///
/// Uses standard ZIP extra field format:
/// - tag: 2 bytes, identifies the extra field type (0x4241 = "BA")
/// - size: 2 bytes, size of data following (4 bytes for u32 ID)
/// - id: 4 bytes, the stable entry ID
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
pub struct BaleExtra {
    /// Extra field tag (0x4241 = "BA" for Bale).
    pub tag: U16<zerocopy::LittleEndian>,
    /// Size of data following (always 4).
    pub size: U16<zerocopy::LittleEndian>,
    /// Entry ID (stable identifier).
    pub id: U32<zerocopy::LittleEndian>,
}

impl BaleExtra {
    /// Creates a new BaleExtra with the given entry ID.
    #[must_use]
    pub fn new(id: u32) -> Self {
        Self {
            tag: U16::new(BALE_EXTRA_TAG),
            size: U16::new(BALE_EXTRA_DATA_SIZE),
            id: U32::new(id),
        }
    }

    /// Returns the entry ID.
    #[must_use]
    pub fn id(&self) -> u32 {
        self.id.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// BaleExtra has expected size of 8 bytes.
    #[test]
    fn size() {
        assert_eq!(std::mem::size_of::<BaleExtra>(), 8);
    }

    /// BaleExtra fields are correctly initialized.
    #[test]
    fn new() {
        let extra = BaleExtra::new(42);
        assert_eq!(extra.tag.get(), BALE_EXTRA_TAG);
        assert_eq!(extra.size.get(), BALE_EXTRA_DATA_SIZE);
        assert_eq!(extra.id(), 42);
    }

    /// BaleExtra serializes to expected bytes.
    #[test]
    fn bytes() {
        let extra = BaleExtra::new(0x12345678);
        let bytes = extra.as_bytes();
        // Tag: 0xBA1D little-endian = [0x1D, 0xBA]
        assert_eq!(bytes[0], 0x1D);
        assert_eq!(bytes[1], 0xBA);
        // Size: 4 little-endian = [0x04, 0x00]
        assert_eq!(bytes[2], 0x04);
        assert_eq!(bytes[3], 0x00);
        // ID: 0x12345678 little-endian = [0x78, 0x56, 0x34, 0x12]
        assert_eq!(bytes[4], 0x78);
        assert_eq!(bytes[5], 0x56);
        assert_eq!(bytes[6], 0x34);
        assert_eq!(bytes[7], 0x12);
    }
}
