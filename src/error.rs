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

    /// Invalid alignment value.
    #[error("invalid alignment: {0}")]
    InvalidAlignment(String),

    /// Path size is outside valid range (1-4096).
    #[error("invalid path size: {0} is not in range 1..=4096")]
    InvalidPathSize(u16),

    /// Path is invalid (traversal, empty, or malformed).
    #[error("invalid path: {0}")]
    InvalidPath(String),

    /// Filename contains unsafe characters or patterns.
    #[error("unsafe filename: {0}")]
    UnsafeFilename(#[from] safename::SafeNameError),

    /// Path contains invalid UTF-8.
    #[error("invalid UTF-8 in path: {0}")]
    InvalidUtf8(#[from] std::str::Utf8Error),

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

    /// Entry exists but is not a file.
    #[error("not a file: {0}")]
    NotAFile(String),

    /// Entry exists but is not a directory.
    #[error("not a directory: {0}")]
    NotADirectory(String),

    /// Entry exists but is not a symlink.
    #[error("not a symlink: {0}")]
    NotASymlink(String),

    /// Archive data is corrupted or invalid.
    #[error("corrupted archive: {0}")]
    Corrupted(String),

    /// Path uses reserved prefix (`.bale`).
    #[error("reserved path: {0}")]
    ReservedPath(String),

    /// Path already exists in the archive.
    #[error("path already exists: {0}")]
    PathExists(String),

    /// Symlink resolution exceeded the maximum depth (cycle detected).
    #[error("symlink loop: {0}")]
    SymlinkLoop(String),

    /// Archive has exhausted its entry ID space (u32::MAX reached).
    #[error("archive is full: entry ID space exhausted")]
    ArchiveFull,

    /// The archive file is locked by another process.
    #[error("file is locked by another process")]
    FileLocked,
}
