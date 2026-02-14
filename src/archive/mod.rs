//! Unified archive access using memory-mapped I/O.
//!
//! This module provides a generic `Archive<M>` struct that works with both
//! read-only and read-write memory maps. The `ArchiveRead` and `ArchiveWrite`
//! traits define the available operations for each mode.
//!
//! # Type Aliases
//!
//! - [`ArchiveReader`] = `Archive<MappedArchive>` - Read-only access
//! - [`ArchiveWriter`] = `Archive<MappedArchiveMut>` - Read-write access

/// Read operations trait.
mod archive_read;
/// Write operations trait.
mod archive_write;
/// Core archive struct.
/// Named `core` instead of `archive` to avoid module inception (clippy::module_inception).
mod core;
/// Directory entry wrapper.
mod dir_entry;
/// Generic entry enum.
mod entry;
/// File entry wrapper.
mod file_entry;
/// Read-only archive implementation.
mod reader;
/// Symlink entry wrapper.
mod symlink_entry;
/// Read-write archive implementation.
mod writer;

pub use archive_read::ArchiveRead;
pub(crate) use archive_read::resolve_target;
pub use archive_write::ArchiveWrite;
pub use core::{Archive, ArchiveReader, ArchiveWriter};
pub use dir_entry::DirEntry;
pub use entry::Entry;
pub use file_entry::FileEntry;
pub use symlink_entry::SymlinkEntry;
