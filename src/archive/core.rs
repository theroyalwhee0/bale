//! Core archive struct with generic memory-map backing.

use crate::format::{EntryRow, Trailer};
use crate::{MappedArchive, MappedArchiveMut};

/// A bale archive with generic memory-map backing.
///
/// This struct provides access to archive contents through memory-mapped I/O.
/// The type parameter `M` determines whether the archive is read-only
/// ([`MappedArchive`]) or read-write ([`MappedArchiveMut`]).
///
/// The entry and directory tables are accessed directly from the mmap via
/// offsets stored in the trailer. The trailer contains all configuration
/// (alignment, path_size) and table locations.
///
/// # Example
///
/// ```ignore
/// use bale::{ArchiveReader, ArchiveRead};
///
/// let reader = ArchiveReader::open("archive.bale")?;
/// for entry in reader.iter_entries() {
///     // process entry...
/// }
/// ```
pub struct Archive<M> {
    /// The memory-mapped archive file.
    pub(super) mmap: M,
    /// Archive trailer containing configuration and table offsets.
    pub(super) trailer: Trailer,
    /// Current write offset (end of file data). Only used for writers.
    pub(super) write_offset: usize,
    /// Whether the archive has been modified since the last sync. Only used for writers.
    pub(super) dirty: bool,
    /// In-memory entry table (used by writers, empty for readers).
    pub(super) entry_rows: Vec<EntryRow>,
    /// In-memory directory table: (null-padded path bytes, entry_id). Used by writers, empty for readers.
    pub(super) dir_entries: Vec<(Vec<u8>, u32)>,
}

/// Read-only archive type alias.
pub type ArchiveReader = Archive<MappedArchive>;

/// Read-write archive type alias.
pub type ArchiveWriter = Archive<MappedArchiveMut>;
