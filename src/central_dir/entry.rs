//! Central Directory entry with parsed header and path.

use super::CentralDirectoryHeader;

/// An in-memory Central Directory entry.
///
/// Holds the header data and null-padded path for an entry.
/// This is internal to the crate.
pub(crate) struct CdEntry {
    /// The Central Directory header.
    pub(crate) header: CentralDirectoryHeader,
    /// The null-padded path (length = path_size).
    pub(crate) path: Vec<u8>,
}
