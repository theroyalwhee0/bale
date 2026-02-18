//! Binary format structures for the bale archive format v1.0.0.
//!
//! This module contains the on-disk layout structures that define the bale
//! archive format. See `docs/bale-spec.md` for the full specification.

/// CRC-32C newtype for strong typing.
mod crc;
/// Directory table row mapping path to entry ID.
mod directory_row;
/// Entry table row (64 bytes) with per-entry metadata.
mod entry_row;
/// File header (8 bytes) at offset 0.
mod file_header;
/// Archive trailer (64 bytes) at end of file.
mod trailer;

pub use crc::Crc;
pub use directory_row::DirectoryRow;
pub use entry_row::{EntryFlags, EntryRow};
pub use file_header::FileHeader;
pub use trailer::{Trailer, TrailerFlags};
