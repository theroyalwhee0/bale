//! Bale archive format library.
//!
//! A mmap-first, zero-copy zip-compatible archive format with fixed-stride
//! entries for efficient random access.
//!
//! # Features
//!
//! - `reader` - Enables [`ArchiveReader`] for reading archives
//! - `writer` - Enables [`ArchiveWriter`] for writing archives
//! - `compact` - Enables [`compact`] and [`rename_duplicates`] functions (requires `reader` + `writer`)
//! - `bin` - Enables the CLI binary (requires `compact`)

/// Validated, normalized paths within a bale archive.
mod archive_path;
/// Central Directory Header for ZIP entries.
mod central_dir;
/// Archive compaction.
#[cfg(feature = "compact")]
mod compact;
/// MS-DOS date/time format for ZIP archives.
mod dos_time;
/// Error types for bale operations.
mod error;
/// Local File Header for ZIP entries.
mod local_file;
/// Memory-mapped file access.
mod mmap;
/// Zero-copy archive reader.
#[cfg(feature = "reader")]
mod reader;
/// Unified archive tail (trailer) structures.
pub mod tail;
/// Append-only archive writer.
#[cfg(feature = "writer")]
mod writer;

pub use archive_path::ArchivePath;
pub use central_dir::CentralDirectoryHeader;
#[cfg(feature = "compact")]
pub use compact::{CompactStats, RenameStats, compact, rename_duplicates};
pub use dos_time::DosDateTime;
pub use error::BaleError;
pub use local_file::LocalFileHeader;
pub use mmap::{MappedArchive, MappedArchiveMut};
#[cfg(feature = "reader")]
pub use reader::ArchiveReader;
pub use tail::{BaleEocd, Eocd, Trailer, Zip64Eocd, Zip64EocdLocator};
#[cfg(feature = "writer")]
pub use writer::ArchiveWriter;
