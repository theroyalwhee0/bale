//! Append-only archive writer using memory-mapped I/O.

use crate::{
    BaleEocd, BaleError, CentralDirectoryHeader, DosDateTime, Eocd, LocalFileHeader,
    MappedArchiveMut,
};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use zerocopy::{FromBytes, IntoBytes};

/// An in-memory Central Directory entry.
///
/// Holds the header data and null-padded path for an entry.
struct CdEntry {
    /// The Central Directory header.
    header: CentralDirectoryHeader,
    /// The null-padded path (length = path_size).
    path: Vec<u8>,
}

/// Append-only archive writer using memory-mapped I/O.
///
/// This writer supports:
/// - Creating new archives
/// - Opening existing archives for appending
/// - Adding entries (new entries shadow duplicates)
/// - Deleting entries (removes from CD, data orphaned until compact)
/// - Syncing to disk (rewrites CD + trailer)
///
/// The Central Directory is kept unsorted until a compact operation.
/// Entries are appended at the end of the file data section.
pub struct ArchiveWriter {
    /// The memory-mapped archive file.
    mmap: MappedArchiveMut,
    /// In-memory Central Directory entries.
    entries: Vec<CdEntry>,
    /// Archive configuration.
    bale_eocd: BaleEocd,
    /// Current write offset (end of file data).
    write_offset: usize,
}

impl ArchiveWriter {
    /// Creates a new empty archive at the given path.
    ///
    /// Uses default settings (4096 alignment, 256 path size).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file cannot be created
    /// - Memory mapping fails
    pub fn create(path: impl AsRef<Path>) -> Result<Self, BaleError> {
        Self::create_with_options(path, 4096, 256)
    }

    /// Creates a new empty archive with custom settings.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the archive file
    /// * `alignment` - Alignment for file data (must be power of 2)
    /// * `path_size` - Maximum path size (1-2048)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file cannot be created
    /// - Memory mapping fails
    /// - Invalid alignment or path size
    pub fn create_with_options(
        path: impl AsRef<Path>,
        alignment: u32,
        path_size: u16,
    ) -> Result<Self, BaleError> {
        let bale_eocd = BaleEocd::new_with_options(alignment, path_size)?;
        let mmap = MappedArchiveMut::create(path)?;

        Ok(Self {
            mmap,
            entries: Vec::new(),
            bale_eocd,
            write_offset: 0,
        })
    }

    /// Opens an existing archive for appending.
    ///
    /// Parses the existing Central Directory to enable shadowing and deletion.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file cannot be opened
    /// - The archive format is invalid
    /// - Memory mapping fails
    pub fn open(path: impl AsRef<Path>) -> Result<Self, BaleError> {
        let mmap = MappedArchiveMut::open(path)?;
        let bytes = mmap.as_bytes();

        // Parse trailer (last 256 bytes).
        if bytes.len() < BaleEocd::COMBINED_SIZE {
            return Err(BaleError::TooSmall {
                size: bytes.len() as u64,
                minimum: BaleEocd::COMBINED_SIZE as u64,
            });
        }

        let trailer_start = bytes.len() - BaleEocd::COMBINED_SIZE;
        let trailer = &bytes[trailer_start..];

        // Parse EOCD.
        let eocd = Eocd::ref_from_bytes(&trailer[..Eocd::SIZE])
            .map_err(|e| BaleError::Corrupted(format!("invalid EOCD: {e}")))?;

        if eocd.signature.get() != Eocd::SIGNATURE {
            return Err(BaleError::InvalidSignature {
                expected: Eocd::SIGNATURE,
                found: eocd.signature.get(),
            });
        }

        // Parse BaleEocd.
        let bale_eocd = BaleEocd::ref_from_bytes(&trailer[Eocd::SIZE..])
            .map_err(|e| BaleError::Corrupted(format!("invalid BaleEocd: {e}")))?;

        if !bale_eocd.is_valid() {
            return Err(BaleError::InvalidSignature {
                expected: BaleEocd::MAGIC,
                found: bale_eocd.magic.get(),
            });
        }

        let bale_eocd = *bale_eocd;
        let path_size = bale_eocd.path_size() as usize;
        let cd_offset = eocd.cd_offset.get() as usize;
        let entry_count = eocd.cd_entries_total.get() as usize;
        let stride = CentralDirectoryHeader::stride(path_size);

        // Parse Central Directory entries.
        let mut entries = Vec::with_capacity(entry_count);
        for i in 0..entry_count {
            let entry_start = cd_offset + i * stride;
            let entry_end = entry_start + stride;
            if entry_end > bytes.len() {
                return Err(BaleError::Corrupted(format!(
                    "CD entry {i} extends beyond archive"
                )));
            }

            let entry_bytes = &bytes[entry_start..entry_end];
            let header = CentralDirectoryHeader::ref_from_bytes(
                &entry_bytes[..CentralDirectoryHeader::SIZE],
            )
            .map_err(|e| BaleError::Corrupted(format!("invalid CD entry {i}: {e}")))?;

            entries.push(CdEntry {
                header: *header,
                path: entry_bytes[CentralDirectoryHeader::SIZE..].to_vec(),
            });
        }

        // Write offset is at the start of the CD.
        let write_offset = cd_offset;

        Ok(Self {
            mmap,
            entries,
            bale_eocd,
            write_offset,
        })
    }

    /// Returns the number of entries in the archive.
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Returns the configured path size for this archive.
    #[must_use]
    pub fn path_size(&self) -> usize {
        self.bale_eocd.path_size() as usize
    }

    /// Returns the configured alignment for this archive.
    ///
    /// # Panics
    ///
    /// Panics if `alignment_pow2` is invalid. This cannot happen for writers
    /// created via [`create()`](Self::create) or [`open()`](Self::open) since
    /// `BaleEocd` validates on construction.
    #[must_use]
    pub fn alignment(&self) -> u32 {
        self.bale_eocd
            .alignment()
            .expect("validated on construction")
    }

    /// Adds an entry from raw data.
    ///
    /// If an entry with the same path already exists, the new entry shadows it.
    /// The old data remains in the archive (orphaned) until compact.
    ///
    /// # Arguments
    ///
    /// * `path` - Archive path for the entry
    /// * `data` - File contents
    /// * `mode` - Unix file permissions (e.g., 0o644)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The path exceeds the archive's path_size
    /// - Writing to the archive fails
    pub fn add_entry(&mut self, path: &str, data: &[u8], mode: u32) -> Result<(), BaleError> {
        let path_bytes = path.as_bytes();
        let path_size = self.path_size();

        if path_bytes.len() > path_size {
            return Err(BaleError::InvalidPathSize(path_bytes.len() as u16));
        }

        // Calculate sizes.
        let alignment = self.alignment() as usize;
        let local_header_stride = LocalFileHeader::stride(path_size);
        let data_size = data.len();

        // Calculate aligned entry size: header + path + data + padding.
        let unaligned_size = local_header_stride + data_size;
        let aligned_size = unaligned_size.div_ceil(alignment) * alignment;
        let padding = aligned_size - unaligned_size;

        // Ensure capacity.
        self.mmap.reserve(aligned_size)?;

        // Build null-padded path.
        let mut padded_path = vec![0u8; path_size];
        padded_path[..path_bytes.len()].copy_from_slice(path_bytes);

        // Compute CRC32.
        let crc = crc32fast::hash(data);

        // Get current time.
        let mtime = DosDateTime::from(std::time::SystemTime::now());

        // Record the local header offset before writing.
        let local_offset = self.write_offset;

        // Build local file header.
        let local_header = LocalFileHeader::new(data_size as u32, crc, mtime, path_size as u16);

        // Write to mmap: header + path + data + padding.
        self.mmap.extend(local_header.as_bytes())?;
        self.mmap.extend(&padded_path)?;
        self.mmap.extend(data)?;
        if padding > 0 {
            let zeros = vec![0u8; padding];
            self.mmap.extend(&zeros)?;
        }

        self.write_offset += aligned_size;

        // Build Central Directory entry.
        let cd_header = CentralDirectoryHeader::new(
            data_size as u32,
            crc,
            mtime,
            local_offset as u32,
            mode,
            path_size as u16,
        );

        self.entries.push(CdEntry {
            header: cd_header,
            path: padded_path,
        });

        Ok(())
    }

    /// Adds a file from the filesystem to the archive.
    ///
    /// # Arguments
    ///
    /// * `src` - Path to the source file
    /// * `archive_path` - Path within the archive
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The source file cannot be read
    /// - The archive path exceeds path_size
    /// - Writing to the archive fails
    pub fn add_file(&mut self, src: impl AsRef<Path>, archive_path: &str) -> Result<(), BaleError> {
        let src = src.as_ref();
        let mut file = File::open(src)?;
        let metadata = file.metadata()?;

        // Read file contents.
        let mut data = Vec::with_capacity(metadata.len() as usize);
        file.read_to_end(&mut data)?;

        // Get Unix mode.
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

        self.add_entry(archive_path, &data, mode)
    }

    /// Deletes an entry by path.
    ///
    /// Removes the entry from the Central Directory. The file data remains
    /// in the archive (orphaned) until a compact operation.
    ///
    /// Returns `true` if an entry was deleted, `false` if not found.
    ///
    /// # Arguments
    ///
    /// * `path` - Archive path to delete
    pub fn delete(&mut self, path: &str) -> bool {
        let path_bytes = path.as_bytes();
        let path_size = self.path_size();

        if path_bytes.len() > path_size {
            return false;
        }

        // Find and remove matching entries (last match for shadowing).
        let initial_len = self.entries.len();
        self.entries.retain(|entry| {
            // Check if path matches (with null padding).
            !(entry.path.starts_with(path_bytes)
                && entry.path[path_bytes.len()..].iter().all(|&b| b == 0))
        });

        self.entries.len() < initial_len
    }

    /// Flushes all changes to disk.
    ///
    /// Rewrites the Central Directory, EOCD, and BaleEocd trailer.
    /// The file is truncated to the logical size.
    ///
    /// # Errors
    ///
    /// Returns an error if writing or syncing fails.
    pub fn sync(&mut self) -> Result<(), BaleError> {
        let path_size = self.path_size();
        let cd_stride = CentralDirectoryHeader::stride(path_size);

        // Calculate CD size.
        let cd_size = self.entries.len() * cd_stride;

        // Calculate total size: file data + CD + EOCD + BaleEocd.
        let cd_offset = self.write_offset;
        let total_size = cd_offset + cd_size + BaleEocd::COMBINED_SIZE;

        // Ensure capacity and set length.
        self.mmap.reserve(cd_size + BaleEocd::COMBINED_SIZE)?;
        self.mmap.set_len(total_size)?;

        let bytes = self.mmap.as_bytes_mut();

        // Write Central Directory entries.
        let mut offset = cd_offset;
        for entry in &self.entries {
            bytes[offset..offset + CentralDirectoryHeader::SIZE]
                .copy_from_slice(entry.header.as_bytes());
            bytes[offset + CentralDirectoryHeader::SIZE..offset + cd_stride]
                .copy_from_slice(&entry.path);
            offset += cd_stride;
        }

        // Write EOCD.
        let eocd = Eocd::new_with_comment(
            self.entries.len() as u16,
            cd_size as u32,
            cd_offset as u32,
            BaleEocd::SIZE as u16,
        );
        bytes[offset..offset + Eocd::SIZE].copy_from_slice(eocd.as_bytes());
        offset += Eocd::SIZE;

        // Write BaleEocd.
        bytes[offset..offset + BaleEocd::SIZE].copy_from_slice(self.bale_eocd.as_bytes());

        // Sync to disk.
        self.mmap.sync()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Creating a new archive works.
    #[test]
    fn create_new_archive() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        let writer = ArchiveWriter::create(&path).unwrap();
        assert_eq!(writer.entry_count(), 0);
        assert_eq!(writer.path_size(), 256);
        assert_eq!(writer.alignment(), 4096);
    }

    /// Adding an entry works.
    #[test]
    fn add_entry_works() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        let mut writer = ArchiveWriter::create(&path).unwrap();
        writer
            .add_entry("hello.txt", b"Hello, World!", 0o644)
            .unwrap();
        assert_eq!(writer.entry_count(), 1);
    }

    /// Syncing writes a valid archive.
    #[test]
    fn sync_creates_valid_archive() {
        use crate::ArchiveReader;

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer
                .add_entry("hello.txt", b"Hello, World!", 0o644)
                .unwrap();
            writer.sync().unwrap();
        }

        // Verify with reader.
        let reader = ArchiveReader::open(&path).unwrap();
        assert_eq!(reader.entry_count(), 1);

        let entry = reader.find_entry("hello.txt").unwrap();
        let data = reader.read_data(entry).unwrap();
        assert_eq!(data, b"Hello, World!");
    }

    /// Adding multiple entries works.
    #[test]
    fn add_multiple_entries() {
        use crate::ArchiveReader;

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("a.txt", b"aaa", 0o644).unwrap();
            writer.add_entry("b.txt", b"bbbbb", 0o644).unwrap();
            writer.add_entry("c.txt", b"ccccccc", 0o644).unwrap();
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&path).unwrap();
        assert_eq!(reader.entry_count(), 3);

        assert_eq!(
            reader
                .read_data(reader.find_entry("a.txt").unwrap())
                .unwrap(),
            b"aaa"
        );
        assert_eq!(
            reader
                .read_data(reader.find_entry("b.txt").unwrap())
                .unwrap(),
            b"bbbbb"
        );
        assert_eq!(
            reader
                .read_data(reader.find_entry("c.txt").unwrap())
                .unwrap(),
            b"ccccccc"
        );
    }

    /// Delete removes entry from CD.
    #[test]
    fn delete_removes_entry() {
        use crate::ArchiveReader;

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("a.txt", b"aaa", 0o644).unwrap();
            writer.add_entry("b.txt", b"bbb", 0o644).unwrap();
            assert!(writer.delete("a.txt"));
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&path).unwrap();
        assert_eq!(reader.entry_count(), 1);
        assert!(reader.find_entry("a.txt").is_none());
        assert!(reader.find_entry("b.txt").is_some());
    }

    /// Opening an existing archive preserves entries.
    #[test]
    fn open_existing_archive() {
        use crate::ArchiveReader;

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        // Create archive.
        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("first.txt", b"first", 0o644).unwrap();
            writer.sync().unwrap();
        }

        // Reopen and add more.
        {
            let mut writer = ArchiveWriter::open(&path).unwrap();
            assert_eq!(writer.entry_count(), 1);
            writer.add_entry("second.txt", b"second", 0o644).unwrap();
            writer.sync().unwrap();
        }

        // Verify.
        let reader = ArchiveReader::open(&path).unwrap();
        assert_eq!(reader.entry_count(), 2);
        assert!(reader.find_entry("first.txt").is_some());
        assert!(reader.find_entry("second.txt").is_some());
    }

    /// Shadow duplicates: new entry shadows old.
    #[test]
    fn shadow_duplicates() {
        use crate::ArchiveReader;

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("file.txt", b"original", 0o644).unwrap();
            writer.add_entry("file.txt", b"updated", 0o644).unwrap();
            writer.sync().unwrap();
        }

        // Both entries exist, but find_entry returns the last one.
        let reader = ArchiveReader::open(&path).unwrap();
        assert_eq!(reader.entry_count(), 2);
        let entry = reader.find_entry("file.txt").unwrap();
        let data = reader.read_data(entry).unwrap();
        assert_eq!(data, b"updated");
    }

    /// add_file works with filesystem files.
    #[test]
    fn add_file_from_filesystem() {
        use crate::ArchiveReader;
        use std::io::Write;

        let dir = TempDir::new().unwrap();
        let archive_path = dir.path().join("test.bale");
        let src_path = dir.path().join("source.txt");

        // Create source file.
        {
            let mut f = File::create(&src_path).unwrap();
            f.write_all(b"file contents").unwrap();
        }

        // Add to archive.
        {
            let mut writer = ArchiveWriter::create(&archive_path).unwrap();
            writer.add_file(&src_path, "source.txt").unwrap();
            writer.sync().unwrap();
        }

        // Verify.
        let reader = ArchiveReader::open(&archive_path).unwrap();
        let entry = reader.find_entry("source.txt").unwrap();
        let data = reader.read_data(entry).unwrap();
        assert_eq!(data, b"file contents");
    }
}
