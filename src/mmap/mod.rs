//! Memory-mapped file access for bale archives.

/// Read-only memory-mapped archive.
mod mapped_archive;
/// Read-write memory-mapped archive.
mod mapped_archive_mut;

pub use mapped_archive::MappedArchive;
pub use mapped_archive_mut::MappedArchiveMut;
