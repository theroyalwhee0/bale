//! Central Directory entry with parsed header and path.

use super::CentralDirectoryHeader;

/// An in-memory Central Directory entry.
///
/// Holds the header data, null-padded path, and stable ID for an entry.
/// This is internal to the crate.
pub(crate) struct CdEntry {
    /// The Central Directory header.
    pub(crate) header: CentralDirectoryHeader,
    /// The null-padded path (length = path_size).
    pub(crate) path: Vec<u8>,
    /// Stable entry ID for persistent identification.
    pub(crate) id: u32,
}
