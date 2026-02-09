//! Extract command implementation.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use bale::format::EntryRow;
use bale::{ArchivePath, ArchiveRead, ArchiveReader, BaleError};

use crate::error::BaleCliError;

/// Extracts entries from an archive.
///
/// If no entries are specified, extracts all entries. Paths are validated
/// via `ArchivePath::normalize()` which rejects traversal attempts (e.g.,
/// `../../../etc/passwd`) and produces safe relative paths.
pub fn run(
    archive_path: impl AsRef<Path>,
    output_dir: impl AsRef<Path>,
    entries: &[String],
) -> Result<(), BaleCliError> {
    let reader = ArchiveReader::open(archive_path)?;
    let output_dir = output_dir.as_ref();

    // Create output directory if it doesn't exist.
    fs::create_dir_all(output_dir)?;

    if entries.is_empty() {
        // Extract all entries in a single pass.
        for (entry_row, path_bytes) in reader.iter_entries() {
            extract_entry(&reader, entry_row, path_bytes, output_dir)?;
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
                extract_entry(&reader, entry_row, path_bytes, output_dir)?;
            }
        }

        // Report any entries that weren't found.
        if let Some(missing) = requested.into_iter().next() {
            return Err(BaleError::EntryNotFound(missing.to_string()).into());
        }
    }

    Ok(())
}

/// Extracts a single entry to the output directory.
///
/// Uses `ArchivePath::normalize()` to validate paths. This rejects any path
/// that attempts to escape via `..` components and ensures the result is a
/// safe relative path (no leading slashes, no `..` components).
fn extract_entry(
    reader: &ArchiveReader,
    entry_row: &EntryRow,
    path_bytes: &[u8],
    output_dir: &Path,
) -> Result<(), BaleCliError> {
    let archive_path = ArchivePath::from_null_padded_bytes(path_bytes);

    // Normalize path: validates UTF-8, rejects `..` traversal, removes
    // leading slashes. The result is a safe relative path.
    let normalized = archive_path.normalize()?;
    let path_str = normalized.as_str().ok_or(BaleError::InvalidPath)?;

    let dest_path = output_dir.join(path_str);

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

    #[allow(clippy::print_stdout)]
    {
        println!("  extracted: {path_str}");
    }

    Ok(())
}
