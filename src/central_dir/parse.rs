//! Central Directory parsing.

use super::{BaleExtra, CdEntry, CentralDirectoryHeader};
use crate::BaleError;
use zerocopy::FromBytes;

/// Parses Central Directory entries from archive bytes.
///
/// # Arguments
///
/// * `bytes` - The full archive bytes
/// * `cd_offset` - Offset to the start of the Central Directory
/// * `entry_count` - Number of entries in the CD
/// * `path_size` - Size of each path field
///
/// # Errors
///
/// Returns an error if any CD entry is malformed or extends beyond the archive.
pub(crate) fn parse_cd_entries(
    bytes: &[u8],
    cd_offset: usize,
    entry_count: usize,
    path_size: usize,
) -> Result<Vec<CdEntry>, BaleError> {
    let stride = CentralDirectoryHeader::stride(path_size);
    let extra_size = CentralDirectoryHeader::EXTRA_SIZE as usize;
    let mut entries = Vec::with_capacity(entry_count);

    for i in 0..entry_count {
        let entry_start = cd_offset + i * stride;
        let entry_end = entry_start + stride;
        if entry_end > bytes.len() {
            return Err(BaleError::Corrupted(format!(
                "CD entry {i} extends beyond archive"
            )));
        }

        let entry_bytes = &bytes[entry_start..entry_end];
        let header =
            CentralDirectoryHeader::ref_from_bytes(&entry_bytes[..CentralDirectoryHeader::SIZE])
                .map_err(|e| BaleError::Corrupted(format!("invalid CD entry {i}: {e}")))?;

        let path_start = CentralDirectoryHeader::SIZE;
        let path_end = path_start + path_size;
        let extra_start = path_end;
        let extra_end = extra_start + extra_size;

        let extra =
            BaleExtra::ref_from_bytes(&entry_bytes[extra_start..extra_end]).map_err(|e| {
                BaleError::Corrupted(format!("invalid extra field in CD entry {i}: {e}"))
            })?;

        entries.push(CdEntry {
            header: *header,
            path: entry_bytes[path_start..path_end].to_vec(),
            id: extra.id(),
        });
    }

    Ok(entries)
}
