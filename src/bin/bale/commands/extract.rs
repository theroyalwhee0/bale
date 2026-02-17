//! Extract command implementation.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use bale::format::EntryRow;
use bale::{ArchivePath, ArchiveRead, ArchiveReader, BaleError, EntryKind};
use dialoguer::Confirm;

use crate::error::BaleCliError;

/// Extracts entries from an archive.
///
/// If no entries are specified, extracts all entries. Paths are validated
/// via `ArchivePath::normalize()` which rejects traversal attempts (e.g.,
/// `../../../etc/passwd`) and produces safe relative paths.
///
/// When `flat` is true, directory components are stripped so all files
/// are extracted directly into the output directory. Directory entries
/// are skipped in flat mode.
///
/// # Errors
///
/// Returns an error if the archive cannot be opened, an entry cannot be
/// read, or file I/O fails during extraction.
pub fn run(
    archive_path: impl AsRef<Path>,
    output_dir: impl AsRef<Path>,
    entries: &[String],
    flat: bool,
) -> Result<(), BaleCliError> {
    let reader = ArchiveReader::open(archive_path)?;
    let output_dir = output_dir.as_ref();

    // Create output directory if it doesn't exist.
    fs::create_dir_all(output_dir)?;

    if entries.is_empty() {
        // Extract all entries in a single pass.
        for (entry_row, path_bytes) in reader.iter_entries() {
            extract_entry(&reader, entry_row, path_bytes, output_dir, flat)?;
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
                extract_entry(&reader, entry_row, path_bytes, output_dir, flat)?;
            }
        }

        // Report any entries that weren't found.
        if let Some(missing) = requested.into_iter().next() {
            return Err(BaleError::EntryNotFound(missing.to_string()).into());
        }
    }

    Ok(())
}

/// Resolves the destination path for an entry, applying flat mode if requested.
///
/// In flat mode, only the file name component is used (directory structure
/// is stripped). Returns `None` for directory entries in flat mode since
/// they have no meaningful file to extract.
///
/// # Errors
///
/// Returns an error if the path cannot be normalized or converted to UTF-8.
fn resolve_dest_path(
    path_bytes: &[u8],
    output_dir: &Path,
    flat: bool,
) -> Result<Option<(PathBuf, String)>, BaleCliError> {
    let archive_path = ArchivePath::from_null_padded_bytes(path_bytes);

    // Normalize path: validates UTF-8, rejects `..` traversal, removes
    // leading slashes. The result is a safe relative path.
    let normalized = archive_path.normalize()?;
    let path_str = normalized
        .as_str()
        .ok_or_else(|| BaleError::InvalidPath(format!("{path_bytes:?}")))?;

    if flat {
        // In flat mode, use only the file name component.
        let file_name = Path::new(path_str)
            .file_name()
            .and_then(|n| n.to_str())
            .map(String::from);

        match file_name {
            Some(name) => {
                let dest_path = output_dir.join(&name);
                Ok(Some((dest_path, name)))
            }
            // Directory entries (e.g., "dir/") have no file name; skip them.
            None => Ok(None),
        }
    } else {
        let dest_path = output_dir.join(path_str);
        Ok(Some((dest_path, path_str.to_string())))
    }
}

/// Prompts the user to confirm overwriting an existing file.
///
/// Returns `true` if the user confirms, `false` if declined.
/// When no interactive terminal is available (e.g., piped input),
/// defaults to `false` and logs a warning.
///
/// # Errors
///
/// Returns an error if the terminal prompt fails unexpectedly.
fn confirm_overwrite(path_str: &str) -> Result<bool, BaleCliError> {
    match Confirm::new()
        .with_prompt(format!("overwrite '{path_str}'?"))
        .default(false)
        .interact()
    {
        Ok(confirmed) => Ok(confirmed),
        Err(_) => {
            // No interactive terminal — default to keeping existing file.
            log::warn!("no terminal for overwrite prompt, keeping existing '{path_str}'");
            Ok(false)
        }
    }
}

/// Extracts a single entry to the output directory.
///
/// Handles files, directories, and symlinks based on the entry kind.
/// In flat mode, directory components are stripped and directory entries
/// are skipped. Prompts before overwriting existing files.
///
/// # Errors
///
/// Returns an error if paths cannot be resolved, data cannot be read,
/// or file system operations fail.
fn extract_entry(
    reader: &ArchiveReader,
    entry_row: &EntryRow,
    path_bytes: &[u8],
    output_dir: &Path,
    flat: bool,
) -> Result<(), BaleCliError> {
    let kind = entry_row.kind();

    // In flat mode, skip directory entries entirely.
    if flat && kind.is_directory() {
        return Ok(());
    }

    let Some((dest_path, display_path)) = resolve_dest_path(path_bytes, output_dir, flat)? else {
        // No usable file name (e.g., root directory in flat mode).
        return Ok(());
    };

    // Check for existing file and prompt before overwriting.
    if dest_path.exists() && !confirm_overwrite(&display_path)? {
        #[allow(clippy::print_stdout)]
        {
            println!("  skipped: {display_path}");
        }
        return Ok(());
    }

    match kind {
        EntryKind::File => extract_file(reader, entry_row, &dest_path, &display_path)?,
        EntryKind::Directory => extract_directory(entry_row, &dest_path, &display_path)?,
        EntryKind::Symlink => extract_symlink(reader, entry_row, &dest_path, &display_path)?,
        EntryKind::Other(mode) => {
            #[allow(clippy::print_stdout)]
            {
                println!("  skipped (unsupported type {mode:#o}): {display_path}");
            }
        }
    }

    Ok(())
}

/// Extracts a regular file entry.
///
/// Creates parent directories, writes file data, and sets Unix permissions.
///
/// # Errors
///
/// Returns an error if directories cannot be created, data cannot be read,
/// or the file cannot be written.
fn extract_file(
    reader: &ArchiveReader,
    entry_row: &EntryRow,
    dest_path: &Path,
    display_path: &str,
) -> Result<(), BaleCliError> {
    // Create parent directories.
    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Read and write data.
    let data = reader.read_data(entry_row)?;
    let mut file = File::create(dest_path)?;
    file.write_all(data)?;

    // Set permissions on Unix.
    set_permissions(entry_row, dest_path)?;

    #[allow(clippy::print_stdout)]
    {
        println!("  extracted: {display_path}");
    }

    Ok(())
}

/// Extracts a directory entry.
///
/// Creates the directory and all parent directories, then sets Unix permissions.
///
/// # Errors
///
/// Returns an error if the directory cannot be created.
fn extract_directory(
    entry_row: &EntryRow,
    dest_path: &Path,
    display_path: &str,
) -> Result<(), BaleCliError> {
    fs::create_dir_all(dest_path)?;

    // Set permissions on Unix.
    set_permissions(entry_row, dest_path)?;

    #[allow(clippy::print_stdout)]
    {
        println!("  extracted: {display_path}");
    }

    Ok(())
}

/// Extracts a symlink entry.
///
/// Reads the symlink target from the entry data and creates a symbolic link.
/// On non-Unix platforms, prints a warning and skips the entry.
///
/// # Errors
///
/// Returns an error if the target path is not valid UTF-8, parent
/// directories cannot be created, or the symlink cannot be created.
fn extract_symlink(
    reader: &ArchiveReader,
    entry_row: &EntryRow,
    dest_path: &Path,
    display_path: &str,
) -> Result<(), BaleCliError> {
    let data = reader.read_data(entry_row)?;
    let target = std::str::from_utf8(data)
        .map_err(|_| BaleError::InvalidPath(format!("symlink target for '{display_path}'")))?;

    #[cfg(unix)]
    {
        // Create parent directories.
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Remove existing path if it's a symlink (exists() returns false
        // for broken symlinks, but symlink_metadata catches them).
        if dest_path.symlink_metadata().is_ok() {
            fs::remove_file(dest_path)?;
        }

        std::os::unix::fs::symlink(target, dest_path)?;

        // Skip set_permissions for symlinks: Unix doesn't broadly support
        // lchmod, and symlink permissions follow the target.

        #[allow(clippy::print_stdout)]
        {
            println!("  extracted: {display_path} -> {target}");
        }
    }

    #[cfg(not(unix))]
    {
        let _ = (dest_path, target);
        #[allow(clippy::print_stdout)]
        {
            println!("  skipped (symlinks not supported on this platform): {display_path}");
        }
    }

    Ok(())
}

/// Sets Unix file permissions from the entry mode bits.
///
/// Only the permission bits (lower 12 bits) are applied, skipping
/// entries with mode 0 (unset). This is a no-op on non-Unix platforms.
///
/// # Errors
///
/// Returns an error if permissions cannot be set.
fn set_permissions(entry_row: &EntryRow, dest_path: &Path) -> Result<(), BaleCliError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = entry_row.mode.get();
        if mode != 0 {
            // Apply only permission bits (mask out file type bits).
            let perm_bits = mode & 0o7777;
            fs::set_permissions(dest_path, fs::Permissions::from_mode(perm_bits))?;
        }
    }

    #[cfg(not(unix))]
    {
        let _ = (entry_row, dest_path);
    }

    Ok(())
}
