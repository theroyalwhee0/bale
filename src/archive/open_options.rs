//! Options for opening an existing archive.

/// Options for opening an existing archive.
///
/// Controls validation behavior when opening archives. By default, metadata
/// CRC-32C is validated to detect corruption. Disable CRC validation for
/// repair scenarios where you need to open a corrupted archive.
#[derive(Debug, Clone)]
pub struct OpenOptions {
    /// Whether to validate the metadata CRC-32C on open.
    ///
    /// When `true` (the default), opening a corrupted archive returns
    /// [`BaleError::Corrupted`](crate::BaleError::Corrupted). Set to `false`
    /// for repair scenarios.
    pub validate_crc: bool,
}

impl Default for OpenOptions {
    /// Returns default options with CRC validation enabled.
    fn default() -> Self {
        Self { validate_crc: true }
    }
}
