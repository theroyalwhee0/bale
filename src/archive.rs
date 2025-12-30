use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;

use zerocopy::IntoBytes;

use crate::{
    ALIGNMENT, BaleError, CentralDirectoryHeader, DosDateTime, Eocd, LocalFileHeader, PATH_SIZE,
};

/// Metadata for an entry in the archive.
struct EntryInfo {
    /// Path within the archive (null-padded to PATH_SIZE).
    path: [u8; PATH_SIZE],
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
pub struct Archive {
    /// The underlying file.
    file: File,
    /// Current write position.
    position: u64,
    /// Entries added to the archive.
    entries: Vec<EntryInfo>,
}

impl Archive {
    /// Creates a new archive at the given path.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be created.
    pub fn create(path: impl AsRef<Path>) -> Result<Self, BaleError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path.as_ref())?;

        Ok(Self {
            file,
            position: 0,
            entries: Vec::new(),
        })
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
    /// - The archive path exceeds 256 bytes
    /// - The source file cannot be read
    /// - Writing to the archive fails
    pub fn add_file(&mut self, src: impl AsRef<Path>, archive_path: &str) -> Result<(), BaleError> {
        // Validate path length.
        let path_bytes = archive_path.as_bytes();
        if path_bytes.len() > PATH_SIZE {
            return Err(BaleError::PathTooLong {
                path: archive_path.to_string(),
                max: PATH_SIZE,
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

        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode()
        };
        #[cfg(not(unix))]
        let mode = 0o644;

        // Record the local header offset.
        let local_offset = self.position;

        // Compute CRC-32 and read file data.
        let (crc32, data) = Self::read_with_crc(&mut src_file)?;

        // Create null-padded path.
        let mut path_buf = [0u8; PATH_SIZE];
        path_buf[..path_bytes.len()].copy_from_slice(path_bytes);

        // Write Local File Header + filename + data (must be contiguous per ZIP spec).
        let local_header = LocalFileHeader::new(size, crc32, mtime);
        self.file.write_all(local_header.as_bytes())?;
        self.file.write_all(&path_buf)?;
        self.file.write_all(&data)?;
        self.position += LocalFileHeader::STRIDE as u64 + data.len() as u64;

        // Pad after entry to alignment (for next entry).
        let padding = Self::padding_to_alignment(self.position);
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
    pub fn finish(mut self) -> Result<(), BaleError> {
        let cd_offset = self.position;
        let entry_count = self.entries.len() as u16;

        // Write Central Directory headers.
        for entry in &self.entries {
            let cd_header = CentralDirectoryHeader::new(
                entry.size,
                entry.crc32,
                entry.mtime,
                entry.local_offset as u32,
                entry.mode,
            );
            self.file.write_all(cd_header.as_bytes())?;
            self.file.write_all(&entry.path)?;
            self.position += CentralDirectoryHeader::STRIDE as u64;
        }

        let cd_size = self.position - cd_offset;

        // Write EOCD.
        let eocd = Eocd::new(entry_count, cd_size as u32, cd_offset as u32);
        self.file.write_all(eocd.as_bytes())?;

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
    const fn padding_to_alignment(position: u64) -> usize {
        let remainder = position as usize % ALIGNMENT;
        if remainder == 0 {
            0
        } else {
            ALIGNMENT - remainder
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    /// An empty archive should contain only the EOCD (22 bytes).
    #[test]
    fn create_empty_archive() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        let archive = Archive::create(&path).unwrap();
        archive.finish().unwrap();

        let metadata = std::fs::metadata(&path).unwrap();
        assert_eq!(metadata.len(), Eocd::SIZE as u64);
    }

    /// Adding a file should produce an archive larger than just EOCD.
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
        assert!(metadata.len() > Eocd::SIZE as u64);
    }

    /// Paths exceeding PATH_SIZE bytes should return an error.
    #[test]
    fn path_too_long_error() {
        let dir = TempDir::new().unwrap();
        let archive_path = dir.path().join("test.bale");
        let file_path = dir.path().join("hello.txt");

        File::create(&file_path).unwrap();

        let mut archive = Archive::create(&archive_path).unwrap();
        let long_path = "a".repeat(PATH_SIZE + 1);
        let result = archive.add_file(&file_path, &long_path);

        assert!(matches!(result, Err(BaleError::PathTooLong { .. })));
    }
}
