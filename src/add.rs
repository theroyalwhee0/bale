//! Adding files from disk into an archive.

use std::path::{Path, PathBuf};

use crate::{ArchivePath, ArchiveWrite, ArchiveWriter, BaleError};

/// Adds files to an existing archive.
///
/// Each file is placed under `prefix` in the archive using its filename.
/// The archive must already exist (use `ArchiveWriter::create` to create
/// a new one). Returns a list of the archive paths that were added.
///
/// # Errors
///
/// Returns an error if:
/// - The archive cannot be opened
/// - A file cannot be read
/// - A constructed archive path is invalid
/// - Writing to the archive fails
pub fn add(
    archive_path: impl AsRef<Path>,
    prefix: &str,
    files: &[PathBuf],
) -> Result<Vec<String>, BaleError> {
    let mut writer = ArchiveWriter::open(archive_path)?;
    let mut added = Vec::new();

    for file in files {
        let name = file.file_name().unwrap_or(file.as_os_str());
        let dest = Path::new(prefix).join(name);
        let entry_path = ArchivePath::try_from(dest)?;
        let entry_str = entry_path.as_str().ok_or(BaleError::InvalidPath)?;

        writer.add_file(file, entry_str)?;
        added.push(entry_str.to_owned());
    }

    writer.sync()?;

    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ArchiveRead, ArchiveReader, ArchiveWriter};
    use tempfile::TempDir;

    /// Adding files to an archive.
    #[test]
    fn add_files() {
        let dir = TempDir::new().unwrap();
        let archive_path = dir.path().join("test.bale");

        // Create empty archive.
        {
            let mut writer = ArchiveWriter::create(&archive_path).unwrap();
            writer.sync().unwrap();
        }

        // Create source files.
        let src_a = dir.path().join("a.txt");
        let src_b = dir.path().join("b.txt");
        std::fs::write(&src_a, b"hello").unwrap();
        std::fs::write(&src_b, b"world").unwrap();

        let added = add(&archive_path, "", &[src_a, src_b]).unwrap();
        assert_eq!(added, vec!["a.txt", "b.txt"]);

        // Verify archive contents.
        let reader = ArchiveReader::open(&archive_path).unwrap();
        assert_eq!(reader.entry_count(), 2);

        let entry = reader.find_entry("a.txt").unwrap();
        assert_eq!(reader.read_data(entry).unwrap(), b"hello");

        let entry = reader.find_entry("b.txt").unwrap();
        assert_eq!(reader.read_data(entry).unwrap(), b"world");
    }

    /// Adding files with a prefix.
    #[test]
    fn add_with_prefix() {
        let dir = TempDir::new().unwrap();
        let archive_path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&archive_path).unwrap();
            writer.sync().unwrap();
        }

        let src = dir.path().join("file.txt");
        std::fs::write(&src, b"data").unwrap();

        let added = add(&archive_path, "sub/dir", &[src]).unwrap();
        assert_eq!(added, vec!["sub/dir/file.txt"]);

        let reader = ArchiveReader::open(&archive_path).unwrap();
        let entry = reader.find_entry("sub/dir/file.txt").unwrap();
        assert_eq!(reader.read_data(entry).unwrap(), b"data");
    }

    /// Adding files returns the correct archive paths.
    #[test]
    fn add_returns_correct_paths() {
        let dir = TempDir::new().unwrap();
        let archive_path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&archive_path).unwrap();
            writer.sync().unwrap();
        }

        let src_a = dir.path().join("x.txt");
        let src_b = dir.path().join("y.txt");
        std::fs::write(&src_a, b"x").unwrap();
        std::fs::write(&src_b, b"y").unwrap();

        let added = add(&archive_path, "prefix", &[src_a, src_b]).unwrap();
        assert_eq!(added, vec!["prefix/x.txt", "prefix/y.txt"]);
    }
}
