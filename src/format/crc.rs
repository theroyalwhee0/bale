//! CRC-32 newtype for strong typing.

/// CRC-32 checksum with semantic zero handling.
///
/// Wraps a raw `u32` CRC value. The value `0` represents "no CRC" (used by
/// directories and empty entries), accessed via [`get()`](Self::get) which
/// returns `None` for zero and `Some(value)` for non-zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Crc(u32);

impl Crc {
    /// No CRC (value 0). Used for directories and empty data blocks.
    pub const NONE: Self = Self(0);

    /// Creates a `Crc` from a raw `u32` value.
    #[must_use]
    pub fn new(value: u32) -> Self {
        Self(value)
    }

    /// Computes the CRC-32 checksum of the given data.
    ///
    /// Returns [`Crc::NONE`] for empty data.
    #[must_use]
    pub fn compute(data: &[u8]) -> Self {
        if data.is_empty() {
            return Self::NONE;
        }
        Self(crc32fast::hash(data))
    }

    /// Returns the CRC value, or `None` if absent (zero).
    #[must_use]
    pub fn get(self) -> Option<u32> {
        if self.0 == 0 { None } else { Some(self.0) }
    }

    /// Returns the raw `u32` value for serialization.
    #[must_use]
    pub fn to_u32(self) -> u32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Crc::NONE` has value 0 and `.get()` returns `None`.
    #[test]
    fn none_is_zero() {
        assert_eq!(Crc::NONE.to_u32(), 0);
        assert_eq!(Crc::NONE.get(), None);
    }

    /// Non-zero value is accessible via `.get()`.
    #[test]
    fn non_zero_get() {
        let crc = Crc::new(0xDEAD_BEEF);
        assert_eq!(crc.get(), Some(0xDEAD_BEEF));
        assert_eq!(crc.to_u32(), 0xDEAD_BEEF);
    }

    /// `Crc::compute` on empty data returns `NONE`.
    #[test]
    fn compute_empty() {
        assert_eq!(Crc::compute(b""), Crc::NONE);
    }

    /// `Crc::compute` on non-empty data returns a non-zero CRC.
    #[test]
    fn compute_non_empty() {
        let crc = Crc::compute(b"Hello, World!");
        assert!(crc.get().is_some());
        assert_eq!(crc.to_u32(), crc32fast::hash(b"Hello, World!"));
    }

    /// Equality compares inner values.
    #[test]
    fn equality() {
        assert_eq!(Crc::new(42), Crc::new(42));
        assert_ne!(Crc::new(42), Crc::new(43));
        assert_eq!(Crc::NONE, Crc::new(0));
    }
}
