//! Core archive struct with generic memory-map backing.

use crate::format::{EntryRow, Trailer};
use crate::{MappedArchive, MappedArchiveMut};

/// A bale archive with generic memory-map backing.
///
/// This struct provides access to archive contents through memory-mapped I/O.
/// The type parameter `M` determines whether the archive is read-only
/// ([`MappedArchive`]) or read-write ([`MappedArchiveMut`]).
///
/// For read-only archives, tables are accessed directly from the mmap via
/// offsets stored in the trailer. For read-write archives, tables are loaded
/// into memory (`entry_rows`, `dir_entries`) for modification.
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
    /// Current write offset (end of data region). Only used for writers.
    pub(super) write_offset: usize,
    /// Whether the archive has been modified since the last sync. Only used for writers.
    pub(super) dirty: bool,
    /// In-memory entry rows. Only used for writers.
    pub(super) entry_rows: Vec<EntryRow>,
    /// In-memory directory entries (path_bytes, entry_id). Only used for writers.
    pub(super) dir_entries: Vec<(Vec<u8>, u32)>,
}

/// Read-only archive type alias.
pub type ArchiveReader = Archive<MappedArchive>;

/// Read-write archive type alias.
pub type ArchiveWriter = Archive<MappedArchiveMut>;
