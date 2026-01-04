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

/// Unified archive access (reader and writer).
#[cfg(any(feature = "reader", feature = "writer"))]
mod archive;
/// Validated, normalized paths within a bale archive.
mod archive_path;
/// Central Directory Header for ZIP entries.
mod central_dir;
/// Archive compaction.
#[cfg(feature = "compact")]
mod compact;
/// MS-DOS date/time format for ZIP archives.
mod dos_time;
/// Entry type classification.
mod entry_kind;
/// Error types for bale operations.
mod error;
/// Local File Header for ZIP entries.
mod local_file;
/// Memory-mapped file access.
mod mmap;
/// Unified archive tail (trailer) structures.
pub mod tail;

#[cfg(feature = "reader")]
pub use archive::ArchiveReader;
#[cfg(any(feature = "reader", feature = "writer"))]
pub use archive::{Archive, ArchiveRead, DirEntry, Entry, FileEntry, SymlinkEntry};
#[cfg(feature = "writer")]
pub use archive::{ArchiveWrite, ArchiveWriter};
pub use archive_path::ArchivePath;
pub use central_dir::CentralDirectoryHeader;
#[cfg(feature = "compact")]
pub use compact::{CompactStats, RenameStats, compact, rename_duplicates};
pub use dos_time::DosDateTime;
pub use entry_kind::EntryKind;
pub use error::BaleError;
pub use local_file::LocalFileHeader;
pub use mmap::{MappedArchive, MappedArchiveMut};
pub use tail::{BaleEocd, Eocd, Trailer, Zip64Eocd, Zip64EocdLocator};
