//! Archive compaction to reclaim space from orphaned data.

use crate::format::EntryRow;
use crate::{
    ArchivePath, ArchiveRead, ArchiveReader, ArchiveWrite, ArchiveWriter, BaleError, EntryKind,
};
use nix::sys::stat::SFlag;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Default permission bits for directories (rwxr-xr-x).
const DEFAULT_DIR_PERM: u32 = 0o755;

/// Guard that deletes a temp file on drop unless marked to persist.
struct TempFileGuard {
    /// Path to the temp file.
    path: PathBuf,
    /// If true, the file is kept (renamed to final destination).
    persist: bool,
}

impl TempFileGuard {
    /// Creates a guard for the given path.
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            persist: false,
        }
    }

    /// Marks the file to be persisted (not deleted on drop).
    fn persist(&mut self) {
        self.persist = true;
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        if !self.persist {
            let _ = fs::remove_file(&self.path);
        }
    }
}

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
    let alignment = reader.alignment()?;
    let path_size = reader.path_size() as u16;

    // Collect entries, keeping only the last occurrence of each path (shadowing).
    // We iterate in order and track seen paths to identify duplicates.
    let mut seen_paths: HashSet<Vec<u8>> = HashSet::new();
    let mut entries_to_copy: Vec<_> = Vec::new();
    let mut total_entries = 0usize;

    for (entry_row, path_bytes) in reader.iter_entries() {
        total_entries += 1;
        // Normalize path by trimming null padding for comparison.
        let trimmed: Vec<u8> = path_bytes.iter().copied().take_while(|&b| b != 0).collect();

        // Track all entries, we'll deduplicate later by keeping last occurrence.
        entries_to_copy.push((entry_row, path_bytes.to_vec(), trimmed));
    }

    // Deduplicate: reverse, keep first of each path, reverse back.
    // This keeps the last occurrence of each path (shadowing behavior).
    entries_to_copy.reverse();
    let mut final_entries: Vec<_> = Vec::new();
    for (entry_row, path_bytes, trimmed) in entries_to_copy {
        if seen_paths.insert(trimmed) {
            final_entries.push((entry_row, path_bytes));
        }
    }
    final_entries.reverse();

    // Collect explicit directory paths (without trailing slashes).
    let explicit_dirs: HashSet<Vec<u8>> = final_entries
        .iter()
        .filter(|(entry_row, _)| entry_row.kind() == EntryKind::Directory)
        .map(|(_, path_bytes)| {
            let trimmed: Vec<u8> = path_bytes.iter().copied().take_while(|&b| b != 0).collect();
            // Remove trailing slash if present.
            if trimmed.ends_with(b"/") {
                trimmed[..trimmed.len() - 1].to_vec()
            } else {
                trimmed
            }
        })
        .collect();

    // Collect all implicit directories from file paths.
    let mut missing_dirs: HashSet<Vec<u8>> = HashSet::new();
    for (_, path_bytes) in &final_entries {
        let trimmed: Vec<u8> = path_bytes.iter().copied().take_while(|&b| b != 0).collect();

        // Extract all parent directories from this path.
        let mut parent = trimmed.as_slice();
        while let Some(pos) = parent.iter().rposition(|&b| b == b'/') {
            parent = &parent[..pos];
            if parent.is_empty() {
                break;
            }
            let parent_vec = parent.to_vec();
            if !explicit_dirs.contains(&parent_vec) {
                missing_dirs.insert(parent_vec);
            }
        }
    }

    // Sort by path for binary search.
    final_entries.sort_by(|a, b| a.1.cmp(&b.1));

    let entries_removed = total_entries - final_entries.len();

    // Create temp file in same directory (for atomic rename).
    // TempFileGuard ensures cleanup on error.
    let parent = path.parent().unwrap_or(Path::new("."));
    let temp_path = parent.join(format!(
        ".{}.compact.tmp",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("archive")
    ));
    let mut guard = TempFileGuard::new(temp_path.clone());

    // Write compacted archive.
    {
        let mut writer = ArchiveWriter::create_with_options(&temp_path, alignment, path_size)?;

        // First, add any missing directory entries.
        // Sort them to ensure parent directories come before children.
        let mut missing_dirs_sorted: Vec<_> = missing_dirs.into_iter().collect();
        missing_dirs_sorted.sort();

        for dir_bytes in &missing_dirs_sorted {
            let dir_str = std::str::from_utf8(dir_bytes)?;
            // Use default directory mode (rwxr-xr-x).
            writer.add_folder(dir_str, SFlag::S_IFDIR.bits() | DEFAULT_DIR_PERM)?;
        }

        // Track which entry IDs have been written to preserve hard links.
        // Maps old entry ID → first path written for that ID.
        let mut written_ids: HashMap<u32, String> = HashMap::new();

        for (entry_row, path_bytes) in &final_entries {
            // Get the path as a validated UTF-8 string.
            let archive_path = ArchivePath::from_null_padded_bytes(path_bytes);
            let path_str = archive_path.to_str_checked()?;

            let old_id = entry_row.entry_id.get();

            if let Some(first_path) = written_ids.get(&old_id) {
                // Hard link: reuse existing entry.
                writer.hard_link(first_path, path_str)?;
            } else {
                // First path for this entry ID: write data.
                let data = reader.read_data(entry_row)?;
                let mode = entry_row.mode.get();
                writer.add_entry(path_str, data, mode)?;
                written_ids.insert(old_id, path_str.to_owned());
            }
        }

        writer.sync()?;
    }

    // Get compacted size.
    let compacted_size = fs::metadata(&temp_path)?.len();

    // Atomic rename.
    fs::rename(&temp_path, path)?;
    guard.persist();

    Ok(CompactStats {
        original_size,
        compacted_size,
        entries_removed,
        bytes_reclaimed: original_size.saturating_sub(compacted_size),
    })
}

/// Statistics from a rename duplicates operation.
#[derive(Debug, Clone, Default)]
pub struct RenameStats {
    /// Number of entries renamed.
    pub entries_renamed: usize,
    /// Mapping of old paths to new paths.
    pub renames: Vec<(String, String)>,
}

/// Renames duplicate paths in an archive.
///
/// When multiple entries share the same path, earlier occurrences are renamed
/// with numeric suffixes while the last occurrence keeps the original name.
/// For example: `file.txt` x 3 → `file(1).txt`, `file(2).txt`, `file.txt`
///
/// This preserves shadowing semantics where the last entry "wins".
///
/// # Errors
///
/// Returns an error if:
/// - The archive cannot be opened
/// - The temp file cannot be created
/// - Writing fails
/// - The rename operation fails
pub fn rename_duplicates(path: impl AsRef<Path>) -> Result<RenameStats, BaleError> {
    let path = path.as_ref();

    // Open existing archive for reading.
    let reader = ArchiveReader::open(path)?;
    let alignment = reader.alignment()?;
    let path_size = reader.path_size() as u16;

    // First pass: count occurrences of each path.
    let mut path_counts: HashMap<String, usize> = HashMap::new();
    for (_entry_row, path_bytes) in reader.iter_entries() {
        let archive_path = ArchivePath::from_null_padded_bytes(path_bytes);
        let path_str = archive_path.to_str_checked()?;
        *path_counts.entry(path_str.to_owned()).or_insert(0) += 1;
    }

    // Check if there are any duplicates.
    let has_duplicates = path_counts.values().any(|&count| count > 1);
    if !has_duplicates {
        return Ok(RenameStats::default());
    }

    // Second pass: collect entries with renamed paths.
    // Track current occurrence number for each path.
    let mut path_occurrences: HashMap<String, usize> = HashMap::new();
    let mut entries: Vec<(&EntryRow, String)> = Vec::new();
    let mut renames: Vec<(String, String)> = Vec::new();

    for (entry_row, path_bytes) in reader.iter_entries() {
        let archive_path = ArchivePath::from_null_padded_bytes(path_bytes);
        let original_path = archive_path.to_str_checked()?.to_owned();
        let total_count = path_counts[&original_path];
        let occurrence = {
            let entry = path_occurrences.entry(original_path.clone()).or_insert(0);
            *entry += 1;
            *entry
        };

        // Determine the new path name.
        let new_path = if total_count > 1 && occurrence < total_count {
            // This is a duplicate that's not the last occurrence - rename it.
            let renamed = archive_path.with_suffix(occurrence)?;

            // Verify renamed path fits within path_size.
            if renamed.len() > path_size as usize {
                return Err(BaleError::PathTooLong {
                    path: renamed.to_string(),
                    max: path_size as usize,
                });
            }

            let renamed_str = renamed.to_string();
            renames.push((original_path, renamed_str.clone()));
            renamed_str
        } else {
            // Either not a duplicate, or it's the last occurrence - keep original.
            original_path
        };

        entries.push((entry_row, new_path));
    }

    // Sort entries by the new path for binary search.
    entries.sort_by(|a, b| a.1.cmp(&b.1));

    // Create temp file in same directory (for atomic rename).
    // TempFileGuard ensures cleanup on error.
    let parent = path.parent().unwrap_or(Path::new("."));
    let temp_path = parent.join(format!(
        ".{}.rename.tmp",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("archive")
    ));
    let mut guard = TempFileGuard::new(temp_path.clone());

    // Write archive with renamed entries.
    {
        let mut writer = ArchiveWriter::create_with_options(&temp_path, alignment, path_size)?;

        // Track which entry IDs have been written to preserve hard links.
        // Maps old entry ID → first path written for that ID.
        let mut written_ids: HashMap<u32, String> = HashMap::new();

        for (entry_row, new_path) in &entries {
            let old_id = entry_row.entry_id.get();

            if let Some(first_path) = written_ids.get(&old_id) {
                // Hard link: reuse existing entry.
                writer.hard_link(first_path, new_path)?;
            } else {
                // First path for this entry ID: write data.
                let data = reader.read_data(entry_row)?;
                let mode = entry_row.mode.get();
                writer.add_entry(new_path, data, mode)?;
                written_ids.insert(old_id, new_path.clone());
            }
        }

        writer.sync()?;
    }

    // Atomic rename.
    fs::rename(&temp_path, path)?;
    guard.persist();

    Ok(RenameStats {
        entries_renamed: renames.len(),
        renames,
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
                ArchivePath::from_null_padded_bytes(p)
                    .to_str_checked()
                    .unwrap()
                    .to_owned()
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

        assert_eq!(exec_entry.mode.get(), 0o755);
        assert_eq!(data_entry.mode.get(), 0o644);
    }

    /// Insert suffix before file extension.
    #[test]
    fn insert_suffix_with_extension() {
        let path = ArchivePath::from_bytes(b"file.txt");
        assert_eq!(path.with_suffix(1).unwrap().as_str(), Some("file(1).txt"));

        let path = ArchivePath::from_bytes(b"image.png");
        assert_eq!(path.with_suffix(2).unwrap().as_str(), Some("image(2).png"));

        let path = ArchivePath::from_bytes(b"archive.tar.gz");
        assert_eq!(
            path.with_suffix(3).unwrap().as_str(),
            Some("archive.tar(3).gz")
        );
    }

    /// Insert suffix for files without extension.
    #[test]
    fn insert_suffix_no_extension() {
        let path = ArchivePath::from_bytes(b"README");
        assert_eq!(path.with_suffix(1).unwrap().as_str(), Some("README(1)"));

        let path = ArchivePath::from_bytes(b"Makefile");
        assert_eq!(path.with_suffix(5).unwrap().as_str(), Some("Makefile(5)"));
    }

    /// Insert suffix handles directories with dots correctly.
    #[test]
    fn insert_suffix_directory_with_dot() {
        // Directory has a dot, but file has no extension.
        let path = ArchivePath::from_bytes(b"foo.d/bar");
        assert_eq!(path.with_suffix(1).unwrap().as_str(), Some("foo.d/bar(1)"));

        // Directory has a dot, file has extension.
        let path = ArchivePath::from_bytes(b"foo.d/bar.txt");
        assert_eq!(
            path.with_suffix(2).unwrap().as_str(),
            Some("foo.d/bar(2).txt")
        );

        // Nested directories with dots.
        let path = ArchivePath::from_bytes(b"a.b/c.d/file.ext");
        assert_eq!(
            path.with_suffix(3).unwrap().as_str(),
            Some("a.b/c.d/file(3).ext")
        );
    }

    /// Rename duplicates on archive with no duplicates does nothing.
    #[test]

    fn rename_duplicates_no_duplicates() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("a.txt", b"a", 0o644).unwrap();
            writer.add_entry("b.txt", b"b", 0o644).unwrap();
            writer.sync().unwrap();
        }

        let stats = rename_duplicates(&path).unwrap();
        assert_eq!(stats.entries_renamed, 0);
        assert!(stats.renames.is_empty());

        // Verify archive unchanged.
        let reader = ArchiveReader::open(&path).unwrap();
        assert_eq!(reader.entry_count(), 2);
    }

    /// Rename duplicates renames earlier occurrences.
    #[test]

    fn rename_duplicates_renames_earlier() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        // Create archive with 3 duplicate entries.
        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("file.txt", b"version 1", 0o644).unwrap();
            writer.add_entry("file.txt", b"version 2", 0o644).unwrap();
            writer.add_entry("file.txt", b"version 3", 0o644).unwrap();
            writer.sync().unwrap();
        }

        let stats = rename_duplicates(&path).unwrap();
        assert_eq!(stats.entries_renamed, 2);

        // Verify renames.
        assert!(
            stats
                .renames
                .contains(&("file.txt".to_string(), "file(1).txt".to_string()))
        );
        assert!(
            stats
                .renames
                .contains(&("file.txt".to_string(), "file(2).txt".to_string()))
        );

        // Verify archive has 3 unique entries.
        let reader = ArchiveReader::open(&path).unwrap();
        assert_eq!(reader.entry_count(), 3);

        // Last occurrence keeps original name.
        let entry = reader.find_entry("file.txt").unwrap();
        let data = reader.read_data(entry).unwrap();
        assert_eq!(data, b"version 3");

        // Earlier occurrences are renamed.
        let entry1 = reader.find_entry("file(1).txt").unwrap();
        let data1 = reader.read_data(entry1).unwrap();
        assert_eq!(data1, b"version 1");

        let entry2 = reader.find_entry("file(2).txt").unwrap();
        let data2 = reader.read_data(entry2).unwrap();
        assert_eq!(data2, b"version 2");
    }

    /// Rename duplicates sorts entries after renaming.
    #[test]

    fn rename_duplicates_sorts_entries() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("z.txt", b"z1", 0o644).unwrap();
            writer.add_entry("z.txt", b"z2", 0o644).unwrap();
            writer.add_entry("a.txt", b"a", 0o644).unwrap();
            writer.sync().unwrap();
        }

        rename_duplicates(&path).unwrap();

        // Verify entries are sorted.
        let reader = ArchiveReader::open(&path).unwrap();
        let paths: Vec<String> = reader
            .iter_entries()
            .map(|(_, p)| {
                ArchivePath::from_null_padded_bytes(p)
                    .to_str_checked()
                    .unwrap()
                    .to_owned()
            })
            .collect();

        assert_eq!(paths, vec!["a.txt", "z(1).txt", "z.txt"]);
    }

    /// Compacting preserves hard link relationships.
    #[test]
    fn compact_preserves_hard_links() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        // Create archive with a hard link.
        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer
                .add_entry("original.txt", b"shared data", 0o644)
                .unwrap();
            writer.hard_link("original.txt", "link.txt").unwrap();
            writer.sync().unwrap();
        }

        compact(&path).unwrap();

        // Verify both paths still exist and share the same entry ID.
        let reader = ArchiveReader::open(&path).unwrap();
        let original = reader.find_entry("original.txt").unwrap();
        let link = reader.find_entry("link.txt").unwrap();

        assert_eq!(original.entry_id.get(), link.entry_id.get());
        assert_eq!(reader.read_data(original).unwrap(), b"shared data");
        assert_eq!(reader.read_data(link).unwrap(), b"shared data");
    }

    /// Compacting renumbers entry IDs sequentially starting at 1.
    #[test]
    fn compact_renumbers_entry_ids() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        // Create archive with gaps in entry IDs by adding then shadowing entries.
        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            // Entry ID 1 — will be shadowed.
            writer.add_entry("a.txt", b"old a", 0o644).unwrap();
            // Entry ID 2.
            writer.add_entry("b.txt", b"b data", 0o644).unwrap();
            // Entry ID 3 — shadows entry ID 1.
            writer.add_entry("a.txt", b"new a", 0o644).unwrap();
            writer.sync().unwrap();
        }

        compact(&path).unwrap();

        // After compaction: 2 entries with IDs 1 and 2 (renumbered sequentially).
        let reader = ArchiveReader::open(&path).unwrap();
        assert_eq!(reader.entry_count(), 2);

        let mut ids: Vec<u32> = reader
            .iter_entries()
            .map(|(entry_row, _)| entry_row.entry_id.get())
            .collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids, vec![1, 2]);
    }

    /// Compacting adds implicit directory entries for nested files.
    #[test]
    fn compact_adds_implicit_directories() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        // Create archive with a nested file but no explicit directory entry.
        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("foo/bar.txt", b"data", 0o644).unwrap();
            writer.sync().unwrap();
        }

        compact(&path).unwrap();

        // Verify the implicit directory was created.
        let reader = ArchiveReader::open(&path).unwrap();
        let dir_entry = reader.find_entry("foo");
        assert!(dir_entry.is_some(), "implicit directory 'foo' should exist");

        let dir_row = dir_entry.unwrap();
        assert_eq!(dir_row.kind(), EntryKind::Directory);
    }

    /// Compacting resets next_id in the trailer to N+1.
    #[test]
    fn compact_resets_next_id() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        // Create archive with gaps from shadowing.
        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("a.txt", b"v1", 0o644).unwrap();
            writer.add_entry("b.txt", b"v1", 0o644).unwrap();
            writer.add_entry("a.txt", b"v2", 0o644).unwrap();
            writer.sync().unwrap();
        }

        compact(&path).unwrap();

        // After compaction: 2 entries (IDs 1, 2), so next_id should be 3.
        let reader = ArchiveReader::open(&path).unwrap();
        assert_eq!(reader.entry_count(), 2);
        assert_eq!(reader.trailer().next_id(), 3);
    }

    /// Renaming duplicates preserves hard link relationships.
    #[test]
    fn rename_duplicates_preserves_hard_links() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        // Create archive with hard links and a duplicate path.
        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer
                .add_entry("file.txt", b"first version", 0o644)
                .unwrap();
            writer.hard_link("file.txt", "link.txt").unwrap();
            writer
                .add_entry("file.txt", b"second version", 0o644)
                .unwrap();
            writer.sync().unwrap();
        }

        let stats = rename_duplicates(&path).unwrap();
        assert_eq!(stats.entries_renamed, 1);

        // Verify the renamed entry and its hard link share the same entry ID.
        let reader = ArchiveReader::open(&path).unwrap();

        let renamed = reader.find_entry("file(1).txt").unwrap();
        let link = reader.find_entry("link.txt").unwrap();
        assert_eq!(renamed.entry_id.get(), link.entry_id.get());
        assert_eq!(reader.read_data(renamed).unwrap(), b"first version");
        assert_eq!(reader.read_data(link).unwrap(), b"first version");

        // The last occurrence keeps the original name.
        let original = reader.find_entry("file.txt").unwrap();
        assert_eq!(reader.read_data(original).unwrap(), b"second version");
    }
}
