//! Bale archive format library.
//!
//! A mmap-first, zero-copy archive format with fixed-stride tables for
//! efficient random access.
//!
//! # Features
//!
//! - `reader` - Enables [`ArchiveReader`] for reading archives, plus
//!   [`check`] and [`extract`] functions
//! - `writer` - Enables [`ArchiveWriter`] for writing archives, plus
//!   the [`add`] function
//! - `compact` - Enables [`compact`] and [`rename_duplicates`] functions (requires `reader` + `writer`)
//! - `bin` - Enables the CLI binary (requires `compact`)

/// Adding files from disk into an archive.
#[cfg(feature = "writer")]
mod add;
/// Unified archive access (reader and writer).
#[cfg(any(feature = "reader", feature = "writer"))]
mod archive;
/// Validated, normalized paths within a bale archive.
mod archive_path;
/// Archive integrity checking.
#[cfg(feature = "reader")]
mod check;
/// Archive compaction.
#[cfg(feature = "compact")]
mod compact;
/// Entry type classification.
mod entry_kind;
/// Error types for bale operations.
mod error;
/// Archive extraction to disk.
#[cfg(feature = "reader")]
mod extract;
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

#[cfg(feature = "writer")]
pub use add::add;
#[cfg(feature = "reader")]
pub use archive::ArchiveReader;
#[cfg(any(feature = "reader", feature = "writer"))]
pub use archive::{Archive, ArchiveRead, DirEntry, Entry, FileEntry, SymlinkEntry};
#[cfg(feature = "writer")]
pub use archive::{ArchiveWrite, ArchiveWriter};
pub use archive_path::ArchivePath;
#[cfg(feature = "reader")]
pub use check::{CheckIssue, CheckReport, check};
#[cfg(feature = "compact")]
pub use compact::{CompactStats, RenameStats, compact, rename_duplicates};
pub use entry_kind::EntryKind;
pub use error::BaleError;
#[cfg(feature = "reader")]
pub use extract::extract;
pub use format::{Crc, DirectoryRow, EntryRow, FileHeader, Trailer};
pub use mmap::{MappedArchive, MappedArchiveMut};
