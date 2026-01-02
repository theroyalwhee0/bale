//! Bale archive format library.
//!
//! A mmap-first, zero-copy zip-compatible archive format with fixed-stride
//! entries for efficient random access.

/// Validated, normalized paths within a bale archive.
mod archive_path;
/// Central Directory Header for ZIP entries.
mod central_dir;
/// Archive compaction.
mod compact;
/// MS-DOS date/time format for ZIP archives.
mod dos_time;
/// Error types for bale operations.
mod error;
/// Local File Header for ZIP entries.
mod local_file;
/// Memory-mapped file access.
mod mmap;
/// Zero-copy archive reader.
mod reader;
/// Unified archive tail (trailer) structures.
pub mod tail;
/// Append-only archive writer.
mod writer;

pub use archive_path::ArchivePath;
pub use central_dir::CentralDirectoryHeader;
pub use compact::{CompactStats, RenameStats, compact, rename_duplicates};
pub use dos_time::DosDateTime;
pub use error::BaleError;
pub use local_file::LocalFileHeader;
pub use mmap::{MappedArchive, MappedArchiveMut};
pub use reader::ArchiveReader;
pub use tail::{BaleEocd, Eocd, Trailer, Zip64Eocd, Zip64EocdLocator};
pub use writer::ArchiveWriter;
