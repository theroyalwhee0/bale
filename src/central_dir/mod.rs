//! Central Directory structures and parsing.

/// Central Directory entry with parsed header and path.
mod entry;
/// Bale extra field for entry IDs.
mod extra;
/// Central Directory File Header.
mod header;
/// Central Directory parsing.
mod parse;

pub(crate) use entry::CdEntry;
pub(crate) use extra::BaleExtra;
pub use header::CentralDirectoryHeader;
pub(crate) use parse::parse_cd_entries;
