//! Binary format structures for the bale archive format v1.0.0.
//!
//! This module contains the on-disk layout structures that define the bale
//! archive format. See `docs/format.md` for the full specification.

/// Data block header (32 bytes) preceding each data block.
mod data_block_header;
/// Directory table row mapping path to entry ID.
mod directory_row;
/// Entry table row (48 bytes) with per-entry metadata.
mod entry_row;
/// File header (8 bytes) at offset 0.
mod file_header;
/// Archive trailer (64 bytes) at end of file.
mod trailer;

pub use data_block_header::DataBlockHeader;
pub use directory_row::DirectoryRow;
pub use entry_row::EntryRow;
pub use file_header::FileHeader;
pub use trailer::Trailer;
