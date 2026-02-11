//! Bale archive format library.
//!
//! A mmap-first, zero-copy archive format with fixed-stride tables for
//! efficient random access.
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
/// Archive compaction.
#[cfg(feature = "compact")]
mod compact;
/// Entry type classification.
mod entry_kind;
/// Error types for bale operations.
mod error;
/// Binary format structures for the bale archive format v1.0.0.
pub mod format;
/// FUSE filesystem support.
#[cfg(feature = "fuse")]
pub mod fuse;
/// Memory-mapped file access.
mod mmap;
/// Proptest configuration (test-only).
#[cfg(test)]
mod proptest_config;

#[cfg(feature = "reader")]
pub use archive::ArchiveReader;
#[cfg(any(feature = "reader", feature = "writer"))]
pub use archive::{Archive, ArchiveRead, DirEntry, Entry, FileEntry, SymlinkEntry};
#[cfg(feature = "writer")]
pub use archive::{ArchiveWrite, ArchiveWriter};
pub use archive_path::ArchivePath;
#[cfg(feature = "compact")]
pub use compact::{CompactStats, RenameStats, compact, rename_duplicates};
pub use entry_kind::EntryKind;
pub use error::BaleError;
pub use format::{Crc, DirectoryRow, EntryRow, FileHeader, Trailer};
pub use mmap::{MappedArchive, MappedArchiveMut};
