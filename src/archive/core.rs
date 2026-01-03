//! Core archive struct with generic memory-map backing.

use super::CdEntry;
use crate::{BaleEocd, MappedArchive, MappedArchiveMut};

/// A bale archive with generic memory-map backing.
///
/// This struct provides access to archive contents through memory-mapped I/O.
/// The type parameter `M` determines whether the archive is read-only
/// ([`MappedArchive`]) or read-write ([`MappedArchiveMut`]).
///
/// # Example
///
/// ```ignore
/// use bale::{ArchiveReader, ArchiveRead};
///
/// let reader = ArchiveReader::open("archive.bale")?;
/// for (header, path) in reader.iter_entries() {
///     let data = reader.read_data(header)?;
///     // process data...
/// }
/// ```
pub struct Archive<M> {
    /// The memory-mapped archive file.
    pub(super) mmap: M,
    /// In-memory Central Directory entries.
    pub(super) entries: Vec<CdEntry>,
    /// Archive configuration from the BaleEocd.
    pub(super) bale_eocd: BaleEocd,
    /// Current write offset (end of file data). Only used for writers.
    pub(super) write_offset: usize,
    /// Whether the archive has been modified since the last sync. Only used for writers.
    pub(super) dirty: bool,
}

/// Read-only archive type alias.
pub type ArchiveReader = Archive<MappedArchive>;

/// Read-write archive type alias.
pub type ArchiveWriter = Archive<MappedArchiveMut>;
