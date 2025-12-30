use thiserror::Error;

/// Errors that can occur when working with bale archives.
#[derive(Error, Debug)]
pub enum BaleError {
    /// I/O error during file operations.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Invalid magic signature in archive header.
    #[error("invalid signature: expected 0x{expected:08X}, found 0x{found:08X}")]
    InvalidSignature {
        /// Expected signature value.
        expected: u32,
        /// Actual signature found.
        found: u32,
    },

    /// Archive path exceeds maximum length.
    #[error("path too long: {path:?} exceeds {max} bytes")]
    PathTooLong {
        /// The path that was too long.
        path: String,
        /// Maximum allowed path length.
        max: usize,
    },
}
