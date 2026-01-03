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

use crate::{BaleError, CentralDirectoryHeader};
use zerocopy::FromBytes;

/// An in-memory Central Directory entry.
///
/// Holds the header data and null-padded path for an entry.
/// This is internal to the archive module.
pub(super) struct CdEntry {
    /// The Central Directory header.
    pub(super) header: CentralDirectoryHeader,
    /// The null-padded path (length = path_size).
    pub(super) path: Vec<u8>,
}

/// Parses Central Directory entries from archive bytes.
///
/// # Arguments
///
/// * `bytes` - The full archive bytes
/// * `cd_offset` - Offset to the start of the Central Directory
/// * `entry_count` - Number of entries in the CD
/// * `path_size` - Size of each path field
///
/// # Errors
///
/// Returns an error if any CD entry is malformed or extends beyond the archive.
pub(super) fn parse_cd_entries(
    bytes: &[u8],
    cd_offset: usize,
    entry_count: usize,
    path_size: usize,
) -> Result<Vec<CdEntry>, BaleError> {
    let stride = CentralDirectoryHeader::stride(path_size);
    let mut entries = Vec::with_capacity(entry_count);

    for i in 0..entry_count {
        let entry_start = cd_offset + i * stride;
        let entry_end = entry_start + stride;
        if entry_end > bytes.len() {
            return Err(BaleError::Corrupted(format!(
                "CD entry {i} extends beyond archive"
            )));
        }

        let entry_bytes = &bytes[entry_start..entry_end];
        let header =
            CentralDirectoryHeader::ref_from_bytes(&entry_bytes[..CentralDirectoryHeader::SIZE])
                .map_err(|e| BaleError::Corrupted(format!("invalid CD entry {i}: {e}")))?;

        entries.push(CdEntry {
            header: *header,
            path: entry_bytes[CentralDirectoryHeader::SIZE..].to_vec(),
        });
    }

    Ok(entries)
}

/// Converts a null-padded path to a string.
pub(super) fn path_to_string(path_bytes: &[u8]) -> String {
    let end = path_bytes
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(path_bytes.len());
    String::from_utf8_lossy(&path_bytes[..end]).to_string()
}
