//! Read operations trait for archives.

use crate::archive::{DirEntry, Entry, FileEntry, SymlinkEntry};
use crate::format::{EntryRow, Trailer};
use crate::{ArchivePath, BaleError};

/// Maximum number of symlink resolutions before declaring a loop.
const MAX_SYMLINK_DEPTH: u32 = 256;

/// Resolves a symlink target path relative to the symlink's location.
///
/// Joins the symlink's parent directory with the target and normalizes
/// the result. Each path component is validated with
/// [`safename::validate_file`] to reject control characters, leading
/// dashes, and components exceeding `NAME_MAX` (255 bytes).
///
/// # Errors
///
/// Returns [`BaleError::InvalidPath`] if the resolved path escapes the
/// archive root or is otherwise invalid.
/// Returns [`BaleError::UnsafeFilename`] if any component violates safename
/// rules.
/// Returns [`BaleError::InvalidUtf8`] if the symlink path is not valid UTF-8.
pub(crate) fn resolve_target(
    symlink_path: &ArchivePath<'_>,
    target: &str,
) -> Result<String, BaleError> {
    // Reject absolute symlink targets (defense against malicious archives).
    if target.starts_with('/') || target.starts_with('\\') {
        return Err(BaleError::InvalidPath(target.to_string()));
    }

    let parent = symlink_path.parent();
    let joined = if parent.is_empty() {
        target.to_string()
    } else {
        let parent_str = parent.to_str_checked()?;
        format!("{parent_str}/{target}")
    };

    // Normalize the joined path: resolve `.`, `..`, collapse slashes,
    // strip leading/trailing slashes. Each normal component is validated
    // with safename to reject control chars, leading dashes, and
    // components exceeding NAME_MAX (255 bytes).
    let mut components: Vec<&str> = Vec::new();
    for part in joined.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(BaleError::InvalidPath(target.to_string()));
                }
            }
            component => {
                safename::validate_file(component)?;
                components.push(component);
            }
        }
    }

    if components.is_empty() {
        return Err(BaleError::InvalidPath(target.to_string()));
    }

    Ok(components.join("/"))
}

/// Read operations for archives.
///
/// This trait is implemented for all `Archive<M>` where `M` provides byte access.
pub trait ArchiveRead {
    /// Returns the number of entries in the archive.
    fn entry_count(&self) -> usize;

    /// Returns the configured path size for this archive.
    fn path_size(&self) -> usize;

    /// Returns the configured alignment for this archive.
    ///
    /// # Errors
    ///
    /// Returns an error if `alignment_power` is invalid.
    fn alignment(&self) -> Result<u32, BaleError>;

    /// Returns the path for the entry at the given index as a zero-copy `ArchivePath`.
    ///
    /// The returned path borrows directly from the mmap.
    /// Returns `None` if the index is out of bounds.
    fn get_path(&self, index: usize) -> Option<ArchivePath<'_>>;

    /// Returns an iterator over all entry rows.
    ///
    /// Each item is a tuple of (entry_row, path_bytes) where path_bytes is the
    /// null-padded path from the directory table.
    fn iter_entries(&self) -> impl Iterator<Item = (&EntryRow, &[u8])>;

    /// Finds an entry row by path.
    ///
    /// Returns the entry row for the given path, or `None` if not found.
    /// The directory table is sorted by path, enabling binary search.
    fn find_entry(&self, path: &str) -> Option<&EntryRow>;

    /// Finds an entry by path and returns entry row, trimmed path bytes, and ID.
    ///
    /// Like [`find_entry`](Self::find_entry), but also returns the path bytes
    /// from the archive (with null padding removed) and the stable entry ID.
    fn find_entry_with_path(&self, path: &str) -> Option<(&EntryRow, &[u8], u32)>;

    /// Returns a zero-copy slice of the file data for the given entry row.
    ///
    /// # Errors
    ///
    /// Returns an error if the entry's data offset or size is invalid.
    fn read_data(&self, entry: &EntryRow) -> Result<&[u8], BaleError>;

    /// Returns a reference to the archive trailer.
    fn trailer(&self) -> &Trailer;

    /// Verifies the CRC-32C checksum for an entry.
    ///
    /// Reads the entry data and computes its CRC-32C, comparing against the
    /// stored value in the entry row.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The entry data cannot be read
    /// - The computed CRC does not match the stored CRC
    fn verify_crc(&self, entry: &EntryRow) -> Result<(), BaleError>;

    /// Verifies the metadata CRC-32C for the archive.
    ///
    /// Computes the CRC over file header, entry table, directory table,
    /// and trailer bytes 0–59, then compares against the stored value.
    ///
    /// Note: This is also validated on [`open()`](crate::ArchiveReader::open),
    /// so a successfully opened archive always has a valid metadata CRC.
    /// This method is useful for explicit reporting in `bale check`.
    ///
    /// # Errors
    ///
    /// Returns an error if the computed CRC does not match the stored CRC.
    fn verify_metadata_crc(&self) -> Result<(), BaleError>;

    /// Checks if the directory table is sorted by path bytes.
    ///
    /// A sorted directory table enables binary search for entry lookup.
    /// Archives created by the writer are always sorted.
    fn is_sorted(&self) -> bool;

    /// Returns a list of duplicate paths in the archive.
    ///
    /// Duplicate paths occur when the same path appears multiple times in the
    /// directory table (e.g., hard links with the same path are not duplicates
    /// since they share the same entry ID).
    fn find_duplicates(&self) -> Vec<ArchivePath<'static>>;

    /// Checks if the archive contains orphaned data.
    ///
    /// Orphaned data exists when there are data blocks not referenced by any
    /// entry in the entry table.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive metadata is corrupted.
    fn has_orphaned_data(&self) -> Result<bool, BaleError>;

    /// Returns a file entry by path.
    ///
    /// This method provides type-safe access to file entries without manual
    /// kind checking. The path is normalized before lookup.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The path is not found ([`BaleError::EntryNotFound`])
    /// - The entry exists but is not a file ([`BaleError::NotAFile`])
    /// - The entry data cannot be read
    fn file(&self, path: impl AsRef<str>) -> Result<FileEntry<'_>, BaleError>;

    /// Returns a directory entry by path.
    ///
    /// This method provides type-safe access to directory entries without manual
    /// kind checking. The path is normalized before lookup (trailing slashes
    /// are handled automatically).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The path is not found ([`BaleError::EntryNotFound`])
    /// - The entry exists but is not a directory ([`BaleError::NotADirectory`])
    fn folder(&self, path: impl AsRef<str>) -> Result<DirEntry<'_>, BaleError>;

    /// Returns a symlink entry by path.
    ///
    /// This method provides type-safe access to symlink entries without manual
    /// kind checking. The path is normalized before lookup.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The path is not found ([`BaleError::EntryNotFound`])
    /// - The entry exists but is not a symlink ([`BaleError::NotASymlink`])
    /// - The entry data cannot be read
    fn symlink(&self, path: impl AsRef<str>) -> Result<SymlinkEntry<'_>, BaleError>;

    /// Returns any entry by path.
    ///
    /// This method returns a generic [`Entry`] enum that can be matched on to
    /// determine the entry type. Use this when you need to handle any type of
    /// entry, or when you don't know the type ahead of time.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The path is not found ([`BaleError::EntryNotFound`])
    /// - The entry data cannot be read (for files and symlinks)
    fn entry(&self, path: impl AsRef<str>) -> Result<Entry<'_>, BaleError>;

    /// Finds an entry by its stable ID.
    ///
    /// Returns `None` if no entry with the given ID exists.
    fn find_by_id(&self, id: u32) -> Option<Entry<'_>>;

    /// Resolves a path, following symlinks transparently.
    ///
    /// Implements the high-level path-walking resolution algorithm from the
    /// spec (§ Symlink Resolution). Symlinks are followed up to a depth of
    /// 256 before returning [`BaleError::SymlinkLoop`]. Low-level methods
    /// ([`entry`](Self::entry), [`file`](Self::file),
    /// [`symlink`](Self::symlink)) are unaffected.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The path is not found ([`BaleError::EntryNotFound`])
    /// - A non-directory component is used as a directory
    ///   ([`BaleError::NotADirectory`])
    /// - A symlink loop is detected ([`BaleError::SymlinkLoop`])
    /// - The path is invalid ([`BaleError::InvalidPath`])
    fn resolve(&self, path: impl AsRef<str>) -> Result<Entry<'_>, BaleError> {
        let normalized = ArchivePath::try_from(path.as_ref())?;
        // ArchivePath::try_from(&str) always produces valid UTF-8.
        let mut current_path = normalized.as_str().unwrap().to_string();
        let mut depth = 0u32;

        'outer: loop {
            // Fast path: look up the full path.
            match self.entry(&current_path) {
                Ok(Entry::Symlink(symlink)) => {
                    depth += 1;
                    if depth > MAX_SYMLINK_DEPTH {
                        return Err(BaleError::SymlinkLoop(current_path));
                    }
                    let target = symlink
                        .target()
                        .ok_or_else(|| BaleError::InvalidPath(current_path.clone()))?;
                    current_path = resolve_target(symlink.path(), target)?;
                    continue;
                }
                Ok(entry) => return Ok(entry),
                Err(BaleError::EntryNotFound(_)) => {}
                Err(e) => return Err(e),
            }

            // Component walk: split path and resolve front-to-back.
            let components: Vec<&str> = current_path.split('/').collect();
            let mut resolved = String::new();

            for (i, component) in components.iter().enumerate() {
                if resolved.is_empty() {
                    resolved.push_str(component);
                } else {
                    resolved.push('/');
                    resolved.push_str(component);
                }

                let remaining = &components[i + 1..];

                match self.entry(&resolved) {
                    Ok(Entry::Symlink(symlink)) => {
                        depth += 1;
                        if depth > MAX_SYMLINK_DEPTH {
                            return Err(BaleError::SymlinkLoop(current_path));
                        }
                        let target = symlink
                            .target()
                            .ok_or_else(|| BaleError::InvalidPath(resolved.clone()))?;
                        let resolved_target = resolve_target(symlink.path(), target)?;
                        current_path = if remaining.is_empty() {
                            resolved_target
                        } else {
                            format!("{resolved_target}/{}", remaining.join("/"))
                        };
                        continue 'outer;
                    }
                    Ok(entry) if remaining.is_empty() => return Ok(entry),
                    Ok(Entry::File(_)) => {
                        return Err(BaleError::NotADirectory(resolved));
                    }
                    Ok(Entry::Directory(_)) => {
                        // Continue to next component.
                    }
                    Err(BaleError::EntryNotFound(_)) => {
                        return Err(BaleError::EntryNotFound(current_path));
                    }
                    Err(e) => return Err(e),
                }
            }

            return Err(BaleError::Corrupted(
                "component walk exhausted without returning".into(),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// resolve_target rejects targets containing control characters.
    #[test]
    fn resolve_target_rejects_control_chars() {
        let symlink = ArchivePath::try_from("link").unwrap();
        let result = resolve_target(&symlink, "foo\x01bar");
        assert!(matches!(result, Err(BaleError::UnsafeFilename(_))));
    }

    /// resolve_target rejects targets with a leading-dash component.
    #[test]
    fn resolve_target_rejects_leading_dash() {
        let symlink = ArchivePath::try_from("link").unwrap();
        let result = resolve_target(&symlink, "-rf");
        assert!(matches!(result, Err(BaleError::UnsafeFilename(_))));
    }

    /// resolve_target rejects targets with a component exceeding NAME_MAX (255).
    #[test]
    fn resolve_target_rejects_long_component() {
        let symlink = ArchivePath::try_from("link").unwrap();
        let long = "a".repeat(257);
        let result = resolve_target(&symlink, &long);
        assert!(matches!(result, Err(BaleError::UnsafeFilename(_))));
    }

    /// resolve_target allows components with dashes in the middle.
    #[test]
    fn resolve_target_allows_dash_in_middle() {
        let symlink = ArchivePath::try_from("link").unwrap();
        let result = resolve_target(&symlink, "foo/bar-baz");
        assert_eq!(result.unwrap(), "foo/bar-baz");
    }

    /// resolve_target allows a component at the NAME_MAX (255) limit.
    #[test]
    fn resolve_target_allows_max_length_component() {
        let symlink = ArchivePath::try_from("link").unwrap();
        let component = "a".repeat(255);
        let result = resolve_target(&symlink, &component);
        assert_eq!(result.unwrap(), component);
    }
}
