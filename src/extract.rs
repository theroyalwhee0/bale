//! Archive extraction to disk.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use crate::format::EntryRow;
use crate::{ArchivePath, ArchiveRead, ArchiveReader, BaleError};

/// Extracts entries from an archive to an output directory.
///
/// If `entries` is empty, extracts all entries. Otherwise, extracts only
/// the specified paths. Returns a list of extracted path strings.
///
/// Paths are validated via [`ArchivePath::normalize()`] which rejects
/// traversal attempts (e.g., `../../../etc/passwd`) and produces safe
/// relative paths.
///
/// # Errors
///
/// Returns an error if:
/// - The archive cannot be opened or read
/// - A requested entry is not found
/// - Path validation fails
/// - File I/O fails (creating directories, writing files)
pub fn extract(
    archive_path: impl AsRef<Path>,
    output_dir: impl AsRef<Path>,
    entries: &[String],
) -> Result<Vec<String>, BaleError> {
    let reader = ArchiveReader::open(archive_path)?;
    let output_dir = output_dir.as_ref();
    let mut extracted = Vec::new();

    // Create output directory if it doesn't exist.
    fs::create_dir_all(output_dir)?;

    if entries.is_empty() {
        // Extract all entries in a single pass.
        for (entry_row, path_bytes) in reader.iter_entries() {
            let path_str = extract_entry(&reader, entry_row, path_bytes, output_dir)?;
            extracted.push(path_str);
        }
    } else {
        // Build set of requested paths for O(1) lookup.
        let mut requested: HashSet<&str> = entries.iter().map(String::as_str).collect();

        // Single pass through archive, extracting matches.
        for (entry_row, path_bytes) in reader.iter_entries() {
            let archive_path = ArchivePath::from_null_padded_bytes(path_bytes);
            if let Some(path_str) = archive_path.as_str()
                && requested.remove(path_str)
            {
                let extracted_path = extract_entry(&reader, entry_row, path_bytes, output_dir)?;
                extracted.push(extracted_path);
            }
        }

        // Report any entries that weren't found.
        if let Some(missing) = requested.into_iter().next() {
            return Err(BaleError::EntryNotFound(missing.to_string()));
        }
    }

    Ok(extracted)
}

/// Extracts a single entry to the output directory, returning the path string.
///
/// Uses [`ArchivePath::normalize()`] to validate paths. This rejects any path
/// that attempts to escape via `..` components and ensures the result is a
/// safe relative path (no leading slashes, no `..` components).
///
/// # Errors
///
/// Returns an error if path validation or file I/O fails.
fn extract_entry(
    reader: &ArchiveReader,
    entry_row: &EntryRow,
    path_bytes: &[u8],
    output_dir: &Path,
) -> Result<String, BaleError> {
    let archive_path = ArchivePath::from_null_padded_bytes(path_bytes);

    // Normalize path: validates UTF-8, rejects `..` traversal, removes
    // leading slashes. The result is a safe relative path.
    let normalized = archive_path.normalize()?;
    let path_str = normalized
        .as_str()
        .ok_or(BaleError::InvalidPath)?
        .to_owned();

    let dest_path = output_dir.join(&path_str);

    // Create parent directories.
    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Read and write data.
    let data = reader.read_data(entry_row)?;
    let mut file = File::create(&dest_path)?;
    file.write_all(data)?;

    // Set permissions on Unix.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = entry_row.mode.get();
        if mode != 0 {
            fs::set_permissions(&dest_path, fs::Permissions::from_mode(mode))?;
        }
    }

    Ok(path_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ArchiveWrite, ArchiveWriter};
    use tempfile::TempDir;

    /// Extracting all entries from an archive.
    #[test]
    fn extract_all_entries() {
        let dir = TempDir::new().unwrap();
        let archive_path = dir.path().join("test.bale");
        let output_dir = dir.path().join("output");

        {
            let mut writer = ArchiveWriter::create(&archive_path).unwrap();
            writer.add_entry("a.txt", b"hello", 0o644).unwrap();
            writer.add_entry("b.txt", b"world", 0o644).unwrap();
            writer.sync().unwrap();
        }

        let extracted = extract(&archive_path, &output_dir, &[]).unwrap();
        assert_eq!(extracted.len(), 2);

        assert_eq!(fs::read(output_dir.join("a.txt")).unwrap(), b"hello");
        assert_eq!(fs::read(output_dir.join("b.txt")).unwrap(), b"world");
    }

    /// Extracting specific entries from an archive.
    #[test]
    fn extract_specific_entries() {
        let dir = TempDir::new().unwrap();
        let archive_path = dir.path().join("test.bale");
        let output_dir = dir.path().join("output");

        {
            let mut writer = ArchiveWriter::create(&archive_path).unwrap();
            writer.add_entry("a.txt", b"hello", 0o644).unwrap();
            writer.add_entry("b.txt", b"world", 0o644).unwrap();
            writer.add_entry("c.txt", b"skip", 0o644).unwrap();
            writer.sync().unwrap();
        }

        let entries = vec!["a.txt".to_string(), "b.txt".to_string()];
        let extracted = extract(&archive_path, &output_dir, &entries).unwrap();
        assert_eq!(extracted.len(), 2);

        assert_eq!(fs::read(output_dir.join("a.txt")).unwrap(), b"hello");
        assert_eq!(fs::read(output_dir.join("b.txt")).unwrap(), b"world");
        assert!(!output_dir.join("c.txt").exists());
    }

    /// Extracting a missing entry returns an error.
    #[test]
    fn extract_missing_entry_error() {
        let dir = TempDir::new().unwrap();
        let archive_path = dir.path().join("test.bale");
        let output_dir = dir.path().join("output");

        {
            let mut writer = ArchiveWriter::create(&archive_path).unwrap();
            writer.add_entry("a.txt", b"hello", 0o644).unwrap();
            writer.sync().unwrap();
        }

        let entries = vec!["missing.txt".to_string()];
        let result = extract(&archive_path, &output_dir, &entries);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), BaleError::EntryNotFound(_)));
    }

    /// Extraction creates parent directories for nested paths.
    #[test]
    fn extract_creates_parent_dirs() {
        let dir = TempDir::new().unwrap();
        let archive_path = dir.path().join("test.bale");
        let output_dir = dir.path().join("output");

        {
            let mut writer = ArchiveWriter::create(&archive_path).unwrap();
            writer
                .add_entry("foo/bar/baz.txt", b"nested", 0o644)
                .unwrap();
            writer.sync().unwrap();
        }

        let extracted = extract(&archive_path, &output_dir, &[]).unwrap();
        assert_eq!(extracted, vec!["foo/bar/baz.txt"]);
        assert_eq!(
            fs::read(output_dir.join("foo/bar/baz.txt")).unwrap(),
            b"nested"
        );
    }

    /// Extraction preserves Unix permissions.
    #[cfg(unix)]
    #[test]
    fn extract_preserves_unix_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().unwrap();
        let archive_path = dir.path().join("test.bale");
        let output_dir = dir.path().join("output");

        {
            let mut writer = ArchiveWriter::create(&archive_path).unwrap();
            writer.add_entry("exec.sh", b"#!/bin/bash", 0o755).unwrap();
            writer.sync().unwrap();
        }

        extract(&archive_path, &output_dir, &[]).unwrap();

        let metadata = fs::metadata(output_dir.join("exec.sh")).unwrap();
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o755);
    }
}
