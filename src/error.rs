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

    /// Alignment is not a power of 2.
    #[error("invalid alignment: {0} is not a power of 2")]
    InvalidAlignment(u32),

    /// Path size is outside valid range (1-2048).
    #[error("invalid path size: {0} is not in range 1..=2048")]
    InvalidPathSize(u16),

    /// Path contains invalid UTF-8.
    #[error("invalid path: not valid UTF-8")]
    InvalidPath,

    /// Archive is too small to contain a valid trailer.
    #[error("archive too small: {size} bytes, minimum is {minimum}")]
    TooSmall {
        /// Actual size of the archive.
        size: u64,
        /// Minimum size required.
        minimum: u64,
    },

    /// Entry not found in archive.
    #[error("entry not found: {0}")]
    EntryNotFound(String),

    /// Archive data is corrupted or invalid.
    #[error("corrupted archive: {0}")]
    Corrupted(String),

    /// Invalid DOS date/time value.
    #[error("invalid DOS date/time: {0}")]
    InvalidDosDateTime(String),
}
