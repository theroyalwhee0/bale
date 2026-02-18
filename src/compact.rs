//! Archive compaction to reclaim space from orphaned data.

use crate::format::EntryRow;
use crate::{
    AddEntryOptions, ArchivePath, ArchiveRead, ArchiveReader, ArchiveWrite, ArchiveWriter,
    BaleError, EntryKind,
};
use nix::sys::stat::SFlag;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use tempfile::TempPath;

/// Default permission bits for directories (rwxr-xr-x).
const DEFAULT_DIR_PERM: u32 = 0o755;

/// Creates a securely-named temp file path in the given directory.
///
/// Uses `tempfile::Builder` for race-free creation with an unpredictable name,
/// then removes the file so `ArchiveWriter` can create it with `O_CREAT|O_EXCL`.
/// The returned `TempPath` auto-deletes the path on drop for error cleanup.
///
/// # Errors
///
/// Returns an error if temp file creation or removal fails.
fn secure_temp_path(dir: &Path) -> Result<TempPath, BaleError> {
    let named = tempfile::Builder::new()
        .prefix(".bale-")
        .suffix(".tmp")
        .tempfile_in(dir)?;
    let temp_path = named.into_temp_path();
    // Remove so ArchiveWriter can create_new the file atomically.
    fs::remove_file(&temp_path)?;
    Ok(temp_path)
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

    // Create a securely-named temp file in the same directory (for atomic rename).
    // TempPath auto-deletes on drop unless persist() is called.
    let parent = path.parent().unwrap_or(Path::new("."));
    let temp_path_guard = secure_temp_path(parent)?;
    let temp_path = temp_path_guard.to_path_buf();

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
            writer.add_directory(dir_str, SFlag::S_IFDIR.bits() | DEFAULT_DIR_PERM)?;
        }

        // Track original entry IDs to preserve hard link structure.
        // Maps original entry_id -> first path written for that entry.
        let mut written_entries: HashMap<u32, String> = HashMap::new();

        for (entry_row, path_bytes) in &final_entries {
            // Get the path as a validated UTF-8 string.
            let archive_path = ArchivePath::from_null_padded_bytes(path_bytes);
            let path_str = archive_path.to_str_checked()?;

            let entry_id = entry_row.entry_id.get();

            if let Some(first_path) = written_entries.get(&entry_id) {
                // This entry ID was already written — create a hard link.
                writer.hard_link(first_path, path_str)?;
            } else {
                // First occurrence of this entry ID — write normally.
                let data = reader.read_data(entry_row)?;
                let mode = entry_row.mode.get();

                writer.add_entry_with_options(
                    path_str,
                    data,
                    mode,
                    AddEntryOptions {
                        created_time: Some(entry_row.created_time.get()),
                        modified_time: Some(entry_row.modified_time.get()),
                    },
                )?;

                written_entries.insert(entry_id, path_str.to_owned());
            }
        }

        writer.sync()?;
    }

    // Get compacted size.
    let compacted_size = fs::metadata(&temp_path)?.len();

    // Atomic rename to final path (also prevents temp file deletion).
    temp_path_guard.persist(path).map_err(|e| e.error)?;

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

    // Create a securely-named temp file in the same directory (for atomic rename).
    // TempPath auto-deletes on drop unless persist() is called.
    let parent = path.parent().unwrap_or(Path::new("."));
    let temp_path_guard = secure_temp_path(parent)?;
    let temp_path = temp_path_guard.to_path_buf();

    // Write archive with renamed entries.
    {
        let mut writer = ArchiveWriter::create_with_options(&temp_path, alignment, path_size)?;

        for (entry_row, new_path) in &entries {
            let data = reader.read_data(entry_row)?;
            let mode = entry_row.mode.get();
            writer.add_entry_with_options(
                new_path,
                data,
                mode,
                AddEntryOptions {
                    created_time: Some(entry_row.created_time.get()),
                    modified_time: Some(entry_row.modified_time.get()),
                },
            )?;
        }

        writer.sync()?;
    }

    // Atomic rename to final path (also prevents temp file deletion).
    temp_path_guard.persist(path).map_err(|e| e.error)?;

    Ok(RenameStats {
        entries_renamed: renames.len(),
        renames,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use tempfile::TempDir;

    use crate::proptest_config;

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

    /// Compacting 3 entries with the same path keeps only the last one.
    #[test]
    fn compact_all_duplicates() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("file.txt", b"v1", 0o644).unwrap();
            writer.add_entry("file.txt", b"v2", 0o644).unwrap();
            writer.add_entry("file.txt", b"v3", 0o644).unwrap();
            writer.sync().unwrap();
        }

        let stats = compact(&path).unwrap();
        assert_eq!(stats.entries_removed, 2);

        let reader = ArchiveReader::open(&path).unwrap();
        assert_eq!(reader.entry_count(), 1);
        let data = reader
            .read_data(reader.find_entry("file.txt").unwrap())
            .unwrap();
        assert_eq!(data, b"v3");
    }

    /// Hard-linked entries survive compaction with shared entry IDs.
    #[test]
    fn compact_preserves_hard_links() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer
                .add_entry("original.txt", b"shared data", 0o100644)
                .unwrap();
            writer.hard_link("original.txt", "link.txt").unwrap();
            writer.sync().unwrap();
        }

        compact(&path).unwrap();

        // After compaction, both paths should exist with the same data
        // and share the same entry ID (hard link preserved).
        let reader = ArchiveReader::open(&path).unwrap();
        let orig = reader.file("original.txt").unwrap();
        let link = reader.file("link.txt").unwrap();
        assert_eq!(orig.data, b"shared data");
        assert_eq!(link.data, b"shared data");
        assert_eq!(orig.id, link.id, "hard links should share entry ID");
        assert_eq!(reader.entry_count(), 1, "should have single entry row");
    }

    /// Multiple hard links to the same file are preserved through compaction.
    #[test]
    fn compact_preserves_multiple_hard_links() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer
                .add_entry("original.txt", b"shared data", 0o100644)
                .unwrap();
            writer.hard_link("original.txt", "link1.txt").unwrap();
            writer.hard_link("original.txt", "link2.txt").unwrap();
            writer.hard_link("original.txt", "link3.txt").unwrap();
            writer.sync().unwrap();
        }

        compact(&path).unwrap();

        let reader = ArchiveReader::open(&path).unwrap();
        let orig = reader.file("original.txt").unwrap();
        let link1 = reader.file("link1.txt").unwrap();
        let link2 = reader.file("link2.txt").unwrap();
        let link3 = reader.file("link3.txt").unwrap();

        // All links share the same data.
        assert_eq!(orig.data, b"shared data");
        assert_eq!(link1.data, b"shared data");
        assert_eq!(link2.data, b"shared data");
        assert_eq!(link3.data, b"shared data");

        // All links share the same entry ID.
        assert_eq!(orig.id, link1.id);
        assert_eq!(orig.id, link2.id);
        assert_eq!(orig.id, link3.id);

        // Single entry row for all links.
        assert_eq!(reader.entry_count(), 1);
    }

    /// Symlink target and mode are preserved through compaction.
    #[test]
    fn compact_handles_symlinks() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer
                .add_entry("target.txt", b"target data", 0o100644)
                .unwrap();
            writer.add_symlink("mylink", "target.txt", 0o777).unwrap();
            writer.sync().unwrap();
        }

        compact(&path).unwrap();

        let reader = ArchiveReader::open(&path).unwrap();
        let entry = reader.find_entry("mylink").unwrap();
        assert_eq!(entry.kind(), EntryKind::Symlink);
        // Mode should preserve symlink type bits.
        assert_eq!(entry.mode.get() & 0o170000, 0o120000);
        let data = reader.read_data(entry).unwrap();
        assert_eq!(data, b"target.txt");
    }

    /// Compaction creates implicit parent directories for files.
    #[test]
    fn compact_creates_implicit_directories() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            // Add a file without its parent directory.
            writer
                .add_entry("a/b/file.txt", b"nested", 0o100644)
                .unwrap();
            writer.sync().unwrap();
        }

        compact(&path).unwrap();

        let reader = ArchiveReader::open(&path).unwrap();
        // The implicit directories should have been created.
        let dir_a = reader.find_entry("a").unwrap();
        assert_eq!(dir_a.kind(), EntryKind::Directory);
        let dir_ab = reader.find_entry("a/b").unwrap();
        assert_eq!(dir_ab.kind(), EntryKind::Directory);
        // The file should still exist.
        let file = reader.file("a/b/file.txt").unwrap();
        assert_eq!(file.data, b"nested");
    }

    /// Rename duplicates produces `file(1).txt`, `file(2).txt` suffixes.
    #[test]
    fn rename_duplicates_suffix_format() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("doc.md", b"first", 0o644).unwrap();
            writer.add_entry("doc.md", b"second", 0o644).unwrap();
            writer.add_entry("doc.md", b"third", 0o644).unwrap();
            writer.add_entry("doc.md", b"fourth", 0o644).unwrap();
            writer.sync().unwrap();
        }

        let stats = rename_duplicates(&path).unwrap();
        assert_eq!(stats.entries_renamed, 3);

        let reader = ArchiveReader::open(&path).unwrap();
        assert_eq!(reader.entry_count(), 4);
        // Last occurrence keeps the original name.
        assert_eq!(
            reader
                .read_data(reader.find_entry("doc.md").unwrap())
                .unwrap(),
            b"fourth"
        );
        // Earlier occurrences get sequential suffixes.
        assert_eq!(
            reader
                .read_data(reader.find_entry("doc(1).md").unwrap())
                .unwrap(),
            b"first"
        );
        assert_eq!(
            reader
                .read_data(reader.find_entry("doc(2).md").unwrap())
                .unwrap(),
            b"second"
        );
        assert_eq!(
            reader
                .read_data(reader.find_entry("doc(3).md").unwrap())
                .unwrap(),
            b"third"
        );
    }

    /// Compacting preserves created_time and modified_time from original entries.
    #[test]
    fn compact_preserves_timestamps() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        // Create archive with entries and record their timestamps.
        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("a.txt", b"aaa", 0o644).unwrap();
            writer.add_entry("b.txt", b"bbb", 0o644).unwrap();
            writer.sync().unwrap();
        }

        // Read timestamps before compaction.
        let (a_created, a_modified, b_created, b_modified) = {
            let reader = ArchiveReader::open(&path).unwrap();
            let a = reader.find_entry("a.txt").unwrap();
            let b = reader.find_entry("b.txt").unwrap();
            (
                a.created_time.get(),
                a.modified_time.get(),
                b.created_time.get(),
                b.modified_time.get(),
            )
        };

        compact(&path).unwrap();

        // Verify timestamps are preserved.
        let reader = ArchiveReader::open(&path).unwrap();
        let a = reader.find_entry("a.txt").unwrap();
        let b = reader.find_entry("b.txt").unwrap();

        assert_eq!(a.created_time.get(), a_created);
        assert_eq!(a.modified_time.get(), a_modified);
        assert_eq!(b.created_time.get(), b_created);
        assert_eq!(b.modified_time.get(), b_modified);
    }

    /// rename_duplicates preserves timestamps from original entries.
    #[test]
    fn rename_duplicates_preserves_timestamps() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        // Create archive with duplicate entries.
        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("file.txt", b"version 1", 0o644).unwrap();
            writer.add_entry("file.txt", b"version 2", 0o644).unwrap();
            writer.sync().unwrap();
        }

        // Read timestamps before rename (iterate to get both entries).
        let timestamps_before: Vec<(i64, i64)> = {
            let reader = ArchiveReader::open(&path).unwrap();
            reader
                .iter_entries()
                .map(|(row, _)| (row.created_time.get(), row.modified_time.get()))
                .collect()
        };
        assert_eq!(timestamps_before.len(), 2);

        rename_duplicates(&path).unwrap();

        // After rename: file(1).txt has first entry's timestamps,
        // file.txt has second entry's timestamps.
        let reader = ArchiveReader::open(&path).unwrap();
        let renamed = reader.find_entry("file(1).txt").unwrap();
        let original = reader.find_entry("file.txt").unwrap();

        assert_eq!(renamed.created_time.get(), timestamps_before[0].0);
        assert_eq!(renamed.modified_time.get(), timestamps_before[0].1);
        assert_eq!(original.created_time.get(), timestamps_before[1].0);
        assert_eq!(original.modified_time.get(), timestamps_before[1].1);
    }

    proptest! {
        #![proptest_config(proptest_config::config())]

        /// Compact round-trip preserves all entry metadata.
        #[test]
        fn compact_round_trip_preserves_metadata(
            entries in prop::collection::vec(
                (
                    "[a-z]{1,8}\\.[a-z]{1,3}",  // path
                    prop::collection::vec(any::<u8>(), 0..64),  // data
                    prop::sample::select(vec![0o100644u32, 0o100755, 0o100600]),  // mode
                ),
                1..8,
            ),
        ) {
            let dir = TempDir::new().unwrap();
            let path = dir.path().join("test.bale");

            // Create archive with generated entries.
            {
                let mut writer = ArchiveWriter::create(&path).unwrap();
                for (entry_path, data, mode) in &entries {
                    writer.add_entry(entry_path, data, *mode).unwrap();
                }
                writer.sync().unwrap();
            }

            // Collect metadata before compaction (last occurrence of each path wins).
            let metadata_before: HashMap<String, (Vec<u8>, u32, i64, i64)> = {
                let reader = ArchiveReader::open(&path).unwrap();
                let mut map = HashMap::new();
                for (row, path_bytes) in reader.iter_entries() {
                    let p = ArchivePath::from_null_padded_bytes(path_bytes)
                        .to_str_checked()
                        .unwrap()
                        .to_owned();
                    let data = reader.read_data(row).unwrap().to_vec();
                    map.insert(p, (data, row.mode.get(), row.created_time.get(), row.modified_time.get()));
                }
                map
            };

            compact(&path).unwrap();

            // Verify all metadata is preserved.
            let reader = ArchiveReader::open(&path).unwrap();
            for (p, (expected_data, expected_mode, expected_created, expected_modified)) in &metadata_before {
                let row = reader.find_entry(p).unwrap();
                let data = reader.read_data(row).unwrap();
                prop_assert_eq!(data, expected_data.as_slice());
                prop_assert_eq!(row.mode.get(), *expected_mode);
                prop_assert_eq!(row.created_time.get(), *expected_created);
                prop_assert_eq!(row.modified_time.get(), *expected_modified);
            }
        }
    }
}
