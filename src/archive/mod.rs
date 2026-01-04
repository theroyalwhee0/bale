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
/// Read-only archive implementation.
mod reader;
/// Read-write archive implementation.
mod writer;

pub use archive_read::ArchiveRead;
pub use archive_write::ArchiveWrite;
pub use core::{Archive, ArchiveReader, ArchiveWriter};
