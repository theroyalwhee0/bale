//! Bale archive format library.
//!
//! A mmap-first, zero-copy zip-compatible archive format with fixed-stride
//! entries for efficient random access.

/// Archive builder for creating bale archives.
mod archive;
/// Validated, normalized paths within a bale archive.
mod archive_path;
/// Bale-specific EOCD extension.
mod bale_eocd;
/// Central Directory Header for ZIP entries.
mod central_dir;
/// MS-DOS date/time format for ZIP archives.
mod dos_time;
/// End of Central Directory record.
mod eocd;
/// Error types for bale operations.
mod error;
/// Local File Header for ZIP entries.
mod local_file;

pub use archive::Archive;
pub use archive_path::ArchivePath;
pub use bale_eocd::BaleEocd;
pub use central_dir::CentralDirectoryHeader;
pub use dos_time::DosDateTime;
pub use eocd::Eocd;
pub use error::BaleError;
pub use local_file::LocalFileHeader;
