//! Archive compaction to reclaim space from orphaned data.

use crate::{ArchiveReader, ArchiveWriter, BaleError};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// Statistics from a compact operation.
#[derive(Debug, Clone, Copy, Default)]
pub struct CompactStats {
    /// Original archive size in bytes.
    pub original_size: u64,
    /// Compacted archive size in bytes.
    pub compacted_size: u64,
    /// Number of duplicate/shadowed entries removed.
    pub entries_removed: usize,
    /// Bytes reclaimed by compaction.
    pub bytes_reclaimed: u64,
}

/// Compacts an archive, removing orphaned data and duplicate entries.
///
/// This operation:
/// 1. Opens the archive for reading
/// 2. Creates a temp file with a new writer
/// 3. Copies non-duplicate entries (keeping only the last occurrence of each path)
/// 4. Sorts entries by path for efficient binary search
/// 5. Atomically replaces the original file
///
/// # Errors
///
/// Returns an error if:
/// - The archive cannot be opened
/// - The temp file cannot be created
/// - Writing fails
/// - The rename operation fails
pub fn compact(path: impl AsRef<Path>) -> Result<CompactStats, BaleError> {
    let path = path.as_ref();

    // Get original size.
    let original_size = fs::metadata(path)?.len();

    // Open existing archive for reading.
    let reader = ArchiveReader::open(path)?;
    let alignment = reader.alignment();
    let path_size = reader.path_size() as u16;

    // Collect entries, keeping only the last occurrence of each path (shadowing).
    // We iterate in order and track seen paths to identify duplicates.
    let mut seen_paths: HashSet<Vec<u8>> = HashSet::new();
    let mut entries_to_copy: Vec<_> = Vec::new();
    let mut total_entries = 0usize;

    for (header, path_bytes) in reader.iter_entries() {
        total_entries += 1;
        // Normalize path by trimming null padding for comparison.
        let trimmed: Vec<u8> = path_bytes.iter().copied().take_while(|&b| b != 0).collect();

        // Track all entries, we'll deduplicate later by keeping last occurrence.
        entries_to_copy.push((header, path_bytes.to_vec(), trimmed.clone()));
    }

    // Deduplicate: reverse, keep first of each path, reverse back.
    // This keeps the last occurrence of each path (shadowing behavior).
    entries_to_copy.reverse();
    let mut final_entries: Vec<_> = Vec::new();
    for (header, path_bytes, trimmed) in entries_to_copy {
        if seen_paths.insert(trimmed) {
            final_entries.push((header, path_bytes));
        }
    }
    final_entries.reverse();

    // Sort by path for binary search.
    final_entries.sort_by(|a, b| a.1.cmp(&b.1));

    let entries_removed = total_entries - final_entries.len();

    // Create temp file in same directory (for atomic rename).
    let parent = path.parent().unwrap_or(Path::new("."));
    let temp_path = parent.join(format!(
        ".{}.compact.tmp",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("archive")
    ));

    // Write compacted archive.
    {
        let mut writer = ArchiveWriter::create_with_options(&temp_path, alignment, path_size)?;

        for (header, path_bytes) in &final_entries {
            // Read the data from the original archive.
            let data = reader.read_data(header)?;

            // Get the path as a string (trimmed).
            let path_str: String = path_bytes
                .iter()
                .copied()
                .take_while(|&b| b != 0)
                .map(|b| b as char)
                .collect();

            // Get mode from external attributes.
            let mode = header.external_attrs.get() >> 16;

            writer.add_entry(&path_str, data, mode)?;
        }

        writer.sync()?;
    }

    // Get compacted size.
    let compacted_size = fs::metadata(&temp_path)?.len();

    // Atomic rename.
    fs::rename(&temp_path, path)?;

    Ok(CompactStats {
        original_size,
        compacted_size,
        entries_removed,
        bytes_reclaimed: original_size.saturating_sub(compacted_size),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Compacting an empty archive works.
    #[test]
    fn compact_empty_archive() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        // Create empty archive.
        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.sync().unwrap();
        }

        let stats = compact(&path).unwrap();
        assert_eq!(stats.entries_removed, 0);

        // Verify archive is still valid.
        let reader = ArchiveReader::open(&path).unwrap();
        assert_eq!(reader.entry_count(), 0);
    }

    /// Compacting removes shadowed duplicates.
    #[test]
    fn compact_removes_duplicates() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        // Create archive with duplicate entries.
        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("file.txt", b"original", 0o644).unwrap();
            writer.add_entry("file.txt", b"updated", 0o644).unwrap();
            writer.add_entry("other.txt", b"other", 0o644).unwrap();
            writer.sync().unwrap();
        }

        let original_size = fs::metadata(&path).unwrap().len();

        let stats = compact(&path).unwrap();
        assert_eq!(stats.entries_removed, 1); // One duplicate removed.
        assert!(stats.compacted_size < original_size);

        // Verify archive has 2 entries with correct data.
        let reader = ArchiveReader::open(&path).unwrap();
        assert_eq!(reader.entry_count(), 2);

        let entry = reader.find_entry("file.txt").unwrap();
        let data = reader.read_data(entry).unwrap();
        assert_eq!(data, b"updated"); // Latest version kept.
    }

    /// Compacting sorts entries by path.
    #[test]
    fn compact_sorts_entries() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        // Create archive with unsorted entries.
        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("c.txt", b"c", 0o644).unwrap();
            writer.add_entry("a.txt", b"a", 0o644).unwrap();
            writer.add_entry("b.txt", b"b", 0o644).unwrap();
            writer.sync().unwrap();
        }

        compact(&path).unwrap();

        // Verify entries are sorted.
        let reader = ArchiveReader::open(&path).unwrap();
        let paths: Vec<String> = reader
            .iter_entries()
            .map(|(_, p)| {
                p.iter()
                    .copied()
                    .take_while(|&b| b != 0)
                    .map(|b| b as char)
                    .collect()
            })
            .collect();

        assert_eq!(paths, vec!["a.txt", "b.txt", "c.txt"]);
    }

    /// Compacting preserves file permissions.
    #[test]
    fn compact_preserves_mode() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("exec.sh", b"#!/bin/bash", 0o755).unwrap();
            writer.add_entry("data.txt", b"data", 0o644).unwrap();
            writer.sync().unwrap();
        }

        compact(&path).unwrap();

        let reader = ArchiveReader::open(&path).unwrap();
        let exec_entry = reader.find_entry("exec.sh").unwrap();
        let data_entry = reader.find_entry("data.txt").unwrap();

        assert_eq!(exec_entry.external_attrs.get() >> 16, 0o755);
        assert_eq!(data_entry.external_attrs.get() >> 16, 0o644);
    }
}
