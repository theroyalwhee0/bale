use crate::{
    ArchivePath, BaleEocd, BaleError, CentralDirectoryHeader, DosDateTime, Eocd, LocalFileHeader,
};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;
use zerocopy::IntoBytes;

/// Metadata for an entry in the archive.
struct EntryInfo {
    /// Path within the archive (null-padded to path_size).
    path: Vec<u8>,
    /// Offset to the Local File Header.
    local_offset: u64,
    /// File size in bytes.
    size: u32,
    /// CRC-32 checksum.
    crc32: u32,
    /// Modification time.
    mtime: DosDateTime,
    /// Unix file mode.
    mode: u32,
}

/// Builder for creating bale archives.
///
/// Use [`Archive::create`] to start building an archive, [`Archive::add_file`]
/// to add files, and [`Archive::finish`] to finalize.
///
/// # Deprecated
///
/// This legacy builder uses non-mmap I/O. Use [`crate::ArchiveWriter`] instead,
/// which provides mmap-based I/O with file locking, append support, and deletion.
#[deprecated(since = "0.1.0", note = "Use ArchiveWriter instead")]
#[allow(deprecated)]
pub struct Archive {
    /// The underlying file.
    file: File,
    /// Current write position.
    position: u64,
    /// Entries added to the archive.
    entries: Vec<EntryInfo>,
    /// Alignment for file data (power of 2).
    alignment: u32,
    /// Maximum path size in bytes.
    path_size: u16,
}

#[allow(deprecated)]
impl Archive {
    /// Default alignment for file data (4096 bytes).
    pub const DEFAULT_ALIGNMENT: u32 = 4096;

    /// Default maximum path size (256 bytes).
    pub const DEFAULT_PATH_SIZE: u16 = 256;

    /// Creates a new archive at the given path with default settings.
    ///
    /// Uses 4096-byte alignment and 256-byte maximum path size.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be created.
    pub fn create(path: impl AsRef<Path>) -> Result<Self, BaleError> {
        Self::create_with_options(path, Self::DEFAULT_ALIGNMENT, Self::DEFAULT_PATH_SIZE)
    }

    /// Creates a new archive with custom alignment and path size.
    ///
    /// # Arguments
    ///
    /// * `path` - Path where the archive will be created
    /// * `alignment` - Alignment for file data (must be a power of 2)
    /// * `path_size` - Maximum path size (1-2048)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file cannot be created
    /// - Alignment is not a power of 2
    /// - Path size is not in range 1..=2048
    pub fn create_with_options(
        path: impl AsRef<Path>,
        alignment: u32,
        path_size: u16,
    ) -> Result<Self, BaleError> {
        // Validate alignment and path_size by trying to create BaleEocd.
        let _ = BaleEocd::new_with_options(alignment, path_size)?;

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path.as_ref())?;
        Ok(Self {
            file,
            position: Default::default(),
            entries: Default::default(),
            alignment,
            path_size,
        })
    }

    /// Adds a file to the archive using a path for the archive name.
    ///
    /// The file is stored uncompressed (STORE method). This method accepts
    /// a `Path` for the archive name, which is validated and normalized.
    ///
    /// # Arguments
    ///
    /// * `src` - Path to the source file on disk
    /// * `archive_path` - Path to store in the archive (must be valid UTF-8)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The archive path is not valid UTF-8
    /// - The archive path exceeds the configured path size
    /// - The source file cannot be read
    /// - Writing to the archive fails
    pub fn add_file_path(
        &mut self,
        src: impl AsRef<Path>,
        archive_path: impl AsRef<Path>,
    ) -> Result<(), BaleError> {
        let archive_path = ArchivePath::try_from_path(archive_path)?;
        self.add_file(src, archive_path.as_str())
    }

    /// Adds a file to the archive.
    ///
    /// The file is stored uncompressed (STORE method).
    ///
    /// # Arguments
    ///
    /// * `src` - Path to the source file on disk
    /// * `archive_path` - Path to store in the archive
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The archive path exceeds the configured path size
    /// - The source file cannot be read
    /// - Writing to the archive fails
    pub fn add_file(&mut self, src: impl AsRef<Path>, archive_path: &str) -> Result<(), BaleError> {
        // Validate path length.
        let path_bytes = archive_path.as_bytes();
        let path_size = self.path_size as usize;
        if path_bytes.len() > path_size {
            return Err(BaleError::PathTooLong {
                path: archive_path.to_string(),
                max: path_size,
            });
        }
        // Open source file and get metadata.
        let mut src_file = File::open(src.as_ref())?;
        let metadata = src_file.metadata()?;
        let size = metadata.len() as u32;
        let mtime = DosDateTime::from(
            metadata
                .modified()
                .unwrap_or_else(|_| std::time::SystemTime::now()),
        );
        // Get the unix permissions or default them.
        let mode = {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                metadata.permissions().mode()
            }
            #[cfg(not(unix))]
            {
                0o644
            }
        };

        // Record the local header offset.
        let local_offset = self.position;

        // Compute CRC-32 and read file data.
        let (crc32, data) = Self::read_with_crc(&mut src_file)?;

        // Create null-padded path.
        let path_buf = {
            let mut buf = vec![0u8; path_size];
            buf[..path_bytes.len()].copy_from_slice(path_bytes);
            buf
        };

        // Write Local File Header + filename + data (must be contiguous per ZIP spec).
        let local_header = LocalFileHeader::new(size, crc32, mtime, self.path_size);
        self.file.write_all(local_header.as_bytes())?;
        self.file.write_all(&path_buf)?;
        self.file.write_all(&data)?;
        self.position += LocalFileHeader::stride(path_size) as u64 + data.len() as u64;

        // Pad after entry to alignment (for next entry).
        let padding = self.padding_to_alignment(self.position);
        if padding > 0 {
            let zeros = vec![0u8; padding];
            self.file.write_all(&zeros)?;
            self.position += padding as u64;
        }

        // Record entry for central directory.
        self.entries.push(EntryInfo {
            path: path_buf,
            local_offset,
            size,
            crc32,
            mtime,
            mode,
        });

        Ok(())
    }

    /// Finishes writing the archive.
    ///
    /// Writes the Central Directory and End of Central Directory record.
    ///
    /// # Errors
    ///
    /// Returns an error if writing fails.
    ///
    /// # Panics
    ///
    /// Panics if the archive was created with invalid alignment or path_size
    /// (should not happen when using `create` or `create_with_options`).
    pub fn finish(mut self) -> Result<(), BaleError> {
        let cd_offset = self.position;
        let entry_count = self.entries.len() as u16;
        let path_size = self.path_size as usize;

        // Sort entries by path for consistent ordering and binary search.
        self.entries.sort_by(|a, b| a.path.cmp(&b.path));

        // Write Central Directory headers.
        for entry in &self.entries {
            let cd_header = CentralDirectoryHeader::new(
                entry.size,
                entry.crc32,
                entry.mtime,
                entry.local_offset as u32,
                entry.mode,
                self.path_size,
            );
            self.file.write_all(cd_header.as_bytes())?;
            self.file.write_all(&entry.path)?;
            self.position += CentralDirectoryHeader::stride(path_size) as u64;
        }

        let cd_size = self.position - cd_offset;

        // Write EOCD with comment length for BaleEocd.
        let eocd = Eocd::new_with_comment(
            entry_count,
            cd_size as u32,
            cd_offset as u32,
            BaleEocd::SIZE as u16,
        );
        self.file.write_all(eocd.as_bytes())?;

        // Write BaleEocd as the EOCD comment.
        let bale_eocd = BaleEocd::new_with_options(self.alignment, self.path_size)
            .expect("already validated in create_with_options");
        self.file.write_all(bale_eocd.as_bytes())?;
        self.file.flush()?;
        Ok(())
    }

    /// Reads a file and computes its CRC-32.
    fn read_with_crc(file: &mut File) -> io::Result<(u32, Vec<u8>)> {
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;
        let crc = crc32fast::hash(&data);
        Ok((crc, data))
    }

    /// Computes padding needed to reach the next alignment boundary.
    fn padding_to_alignment(&self, position: u64) -> usize {
        let alignment = self.alignment as usize;
        let remainder = position as usize % alignment;
        if remainder == 0 {
            0
        } else {
            alignment - remainder
        }
    }
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    /// An empty archive contains EOCD + BaleEocd (256 bytes total).
    #[test]
    fn create_empty_archive() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");
        let archive = Archive::create(&path).unwrap();
        archive.finish().unwrap();
        let metadata = std::fs::metadata(&path).unwrap();
        assert_eq!(metadata.len(), BaleEocd::COMBINED_SIZE as u64);
    }

    /// Adding a file should produce an archive larger than the trailer.
    #[test]
    fn add_single_file() {
        let dir = TempDir::new().unwrap();
        let archive_path = dir.path().join("test.bale");
        let file_path = dir.path().join("hello.txt");
        let mut f = File::create(&file_path).unwrap();
        f.write_all(b"Hello, World!").unwrap();
        let mut archive = Archive::create(&archive_path).unwrap();
        archive.add_file(&file_path, "hello.txt").unwrap();
        archive.finish().unwrap();
        let metadata = std::fs::metadata(&archive_path).unwrap();
        assert!(metadata.len() > BaleEocd::COMBINED_SIZE as u64);
    }

    /// Paths exceeding the configured path size should return an error.
    #[test]
    fn path_too_long_error() {
        let dir = TempDir::new().unwrap();
        let archive_path = dir.path().join("test.bale");
        let file_path = dir.path().join("hello.txt");
        File::create(&file_path).unwrap();
        let mut archive = Archive::create(&archive_path).unwrap();
        let long_path = "a".repeat(Archive::DEFAULT_PATH_SIZE as usize + 1);
        let result = archive.add_file(&file_path, &long_path);
        assert!(matches!(result, Err(BaleError::PathTooLong { .. })));
    }

    /// Central Directory entries should be sorted by path for binary search.
    #[test]
    fn cd_entries_sorted_by_path() {
        use crate::CentralDirectoryHeader;
        use std::io::{Read, Seek, SeekFrom};
        use zerocopy::FromBytes;

        let dir = TempDir::new().unwrap();
        let archive_path = dir.path().join("test.bale");

        // Create test files.
        for name in ["a.txt", "b.txt", "c.txt"] {
            let mut f = File::create(dir.path().join(name)).unwrap();
            f.write_all(name.as_bytes()).unwrap();
        }

        // Add files in reverse order.
        let mut archive = Archive::create(&archive_path).unwrap();
        archive.add_file(dir.path().join("c.txt"), "c.txt").unwrap();
        archive.add_file(dir.path().join("a.txt"), "a.txt").unwrap();
        archive.add_file(dir.path().join("b.txt"), "b.txt").unwrap();
        archive.finish().unwrap();

        // Read back and verify CD order.
        let mut file = File::open(&archive_path).unwrap();
        let file_len = file.metadata().unwrap().len();

        // Read EOCD to find CD offset.
        let eocd_offset = file_len - BaleEocd::COMBINED_SIZE as u64;
        file.seek(SeekFrom::Start(eocd_offset)).unwrap();
        let mut eocd_buf = [0u8; Eocd::SIZE];
        file.read_exact(&mut eocd_buf).unwrap();
        let eocd = Eocd::ref_from_bytes(&eocd_buf).unwrap();
        let cd_offset = eocd.cd_offset.get() as u64;

        // Read paths from CD entries.
        let path_size = Archive::DEFAULT_PATH_SIZE as usize;
        let stride = CentralDirectoryHeader::stride(path_size);
        let mut paths = Vec::new();
        for i in 0..3 {
            let entry_offset = cd_offset + (i * stride) as u64;
            let path_offset = entry_offset + CentralDirectoryHeader::SIZE as u64;
            file.seek(SeekFrom::Start(path_offset)).unwrap();
            let mut path_buf = vec![0u8; path_size];
            file.read_exact(&mut path_buf).unwrap();
            // Trim null padding.
            let end = path_buf.iter().position(|&b| b == 0).unwrap_or(path_size);
            paths.push(String::from_utf8_lossy(&path_buf[..end]).to_string());
        }

        assert_eq!(paths, vec!["a.txt", "b.txt", "c.txt"]);
    }
}
