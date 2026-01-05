//! CLI command implementations.

/// Add files to an archive.
pub mod add;
/// Check archive integrity.
pub mod check;
/// Compact an archive.
pub mod compact;
/// Delete entries from an archive.
pub mod delete;
/// Extract entries from an archive.
pub mod extract;
/// List entries in an archive.
pub mod list;
/// Mount archive as filesystem.
#[cfg(feature = "fuse")]
pub mod mount;
/// Create or update file modification time.
pub mod touch;
