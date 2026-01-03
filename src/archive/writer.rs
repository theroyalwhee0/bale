//! Read-write archive implementation.

use super::{Archive, ArchiveRead, ArchiveWrite, CdEntry, parse_cd_entries, path_to_string};
use crate::{
    ArchivePath, BaleEocd, BaleError, CentralDirectoryHeader, DosDateTime, Eocd, LocalFileHeader,
    MappedArchiveMut, Trailer, Zip64Eocd,
};
use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use zerocopy::IntoBytes;

/// Pre-allocated zero buffer for padding (avoids allocation for common cases).
///
/// Sized to match the default alignment (4096 bytes). For larger alignments,
/// padding is written in chunks from this buffer.
static ZERO_PAD: [u8; 4096] = [0u8; 4096];

impl Archive<MappedArchiveMut> {
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
            dirty: true, // New archive needs initial sync to write trailer.
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

        // Parse and validate trailer.
        let trailer = Trailer::from_archive_bytes(bytes)?;
        let bale_eocd = trailer.bale_eocd;
        let path_size = trailer.path_size() as usize;
        let cd_offset = trailer.cd_offset() as usize;
        let entry_count = trailer.entry_count() as usize;

        // Parse Central Directory entries.
        let entries = parse_cd_entries(bytes, cd_offset, entry_count, path_size)?;

        Ok(Self {
            mmap,
            entries,
            bale_eocd,
            write_offset: cd_offset,
            dirty: false,
        })
    }

    /// Returns a reference to the parsed trailer.
    #[must_use]
    pub fn trailer(&self) -> Trailer {
        let cd_size = self.entries.len() * CentralDirectoryHeader::stride(self.path_size());
        let zip64_eocd_offset = self.write_offset + cd_size;

        Trailer::new(
            self.entries.len() as u64,
            cd_size as u64,
            self.write_offset as u64,
            zip64_eocd_offset as u64,
            self.bale_eocd,
        )
    }

    /// Returns a reference to the ZIP64 EOCD.
    #[must_use]
    pub fn zip64_eocd(&self) -> Zip64Eocd {
        self.trailer().zip64_eocd
    }

    /// Returns a reference to the EOCD.
    #[must_use]
    pub fn eocd(&self) -> Eocd {
        self.trailer().eocd
    }
}

impl ArchiveRead for Archive<MappedArchiveMut> {
    fn entry_count(&self) -> usize {
        self.entries.len()
    }

    fn path_size(&self) -> usize {
        self.bale_eocd.path_size() as usize
    }

    fn alignment(&self) -> u32 {
        self.bale_eocd
            .alignment()
            .expect("validated on construction")
    }

    fn get_path(&self, index: usize) -> Option<ArchivePath<'_>> {
        let entry = self.entries.get(index)?;
        let path_bytes = &entry.path;

        let end = path_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(path_bytes.len());

        Some(ArchivePath::from_bytes(&path_bytes[..end]))
    }

    fn iter_entries(&self) -> impl Iterator<Item = (&CentralDirectoryHeader, &[u8])> {
        self.entries
            .iter()
            .map(|entry| (&entry.header, entry.path.as_slice()))
    }

    fn find_entry(&self, path: &str) -> Option<&CentralDirectoryHeader> {
        let path_bytes = path.as_bytes();
        let path_size = self.path_size();

        if path_bytes.len() > path_size {
            return None;
        }

        let mut result = None;
        for entry in &self.entries {
            if entry.path.starts_with(path_bytes)
                && entry.path[path_bytes.len()..].iter().all(|&b| b == 0)
            {
                result = Some(&entry.header);
            }
        }
        result
    }

    fn read_data(&self, entry: &CentralDirectoryHeader) -> Result<&[u8], BaleError> {
        let bytes = self.mmap.as_bytes();
        let local_offset = entry.local_header_offset.get() as usize;
        let path_size = self.path_size();
        let data_size = entry.uncompressed_size.get() as usize;

        let local_stride = LocalFileHeader::stride(path_size);
        let data_start = local_offset
            .checked_add(local_stride)
            .ok_or_else(|| BaleError::Corrupted("offset overflow".to_string()))?;
        let data_end = data_start
            .checked_add(data_size)
            .ok_or_else(|| BaleError::Corrupted("size overflow".to_string()))?;

        if data_end > bytes.len() {
            return Err(BaleError::Corrupted(format!(
                "entry data extends beyond archive: offset={local_offset}, size={data_size}"
            )));
        }

        Ok(&bytes[data_start..data_end])
    }

    fn bale_eocd(&self) -> &BaleEocd {
        &self.bale_eocd
    }

    fn verify_crc(&self, entry: &CentralDirectoryHeader) -> Result<(), BaleError> {
        let data = self.read_data(entry)?;
        let computed = crc32fast::hash(data);
        let stored = entry.crc32.get();

        if computed != stored {
            return Err(BaleError::Corrupted(format!(
                "CRC mismatch: expected {:08x}, got {:08x}",
                stored, computed
            )));
        }

        Ok(())
    }

    fn is_sorted(&self) -> bool {
        let mut prev: Option<&[u8]> = None;
        for entry in &self.entries {
            if let Some(p) = prev
                && p > entry.path.as_slice()
            {
                return false;
            }
            prev = Some(&entry.path);
        }
        true
    }

    fn find_duplicates(&self) -> Vec<String> {
        let mut seen: HashSet<&[u8]> = HashSet::new();
        let mut duplicate_set: HashSet<&[u8]> = HashSet::new();

        for entry in &self.entries {
            if !seen.insert(&entry.path) {
                duplicate_set.insert(&entry.path);
            }
        }

        duplicate_set.into_iter().map(path_to_string).collect()
    }

    fn has_orphaned_data(&self) -> bool {
        if self.entries.is_empty() {
            return false;
        }

        let mut sorted_entries: Vec<_> = self.entries.iter().collect();
        sorted_entries.sort_by_key(|e| e.header.local_header_offset.get());

        let path_size = self.path_size();
        let alignment = self.alignment() as usize;
        let local_header_stride = LocalFileHeader::stride(path_size);

        let mut expected_offset: usize = 0;

        for entry in &sorted_entries {
            let local_offset = entry.header.local_header_offset.get() as usize;
            let data_size = entry.header.uncompressed_size.get() as usize;

            if local_offset != expected_offset {
                return true;
            }

            let Some(entry_size) = local_header_stride.checked_add(data_size) else {
                return true;
            };
            let aligned_size = entry_size.div_ceil(alignment).saturating_mul(alignment);
            let Some(next_offset) = local_offset.checked_add(aligned_size) else {
                return true;
            };
            expected_offset = next_offset;
        }

        expected_offset != self.write_offset
    }
}

impl ArchiveWrite for Archive<MappedArchiveMut> {
    fn add_entry(&mut self, path: &str, data: &[u8], mode: u32) -> Result<(), BaleError> {
        let path_bytes = path.as_bytes();
        let path_size = self.path_size();

        if path_bytes.len() > path_size {
            return Err(BaleError::InvalidPathSize(path_bytes.len() as u16));
        }

        let alignment = self.alignment() as usize;
        let local_header_stride = LocalFileHeader::stride(path_size);
        let data_size = data.len();

        let data_size_u32: u32 = data_size.try_into().map_err(|_| {
            BaleError::SizeOverflow(format!(
                "data size {} exceeds maximum of {} bytes",
                data_size,
                u32::MAX
            ))
        })?;

        let local_offset = self.write_offset;
        let local_offset_u32: u32 = local_offset.try_into().map_err(|_| {
            BaleError::SizeOverflow(format!(
                "archive offset {} exceeds maximum of {} bytes",
                local_offset,
                u32::MAX
            ))
        })?;

        let unaligned_size = local_header_stride + data_size;
        let aligned_size = unaligned_size.div_ceil(alignment) * alignment;
        let padding = aligned_size - unaligned_size;

        self.mmap.reserve(aligned_size)?;

        let mut padded_path = vec![0u8; path_size];
        padded_path[..path_bytes.len()].copy_from_slice(path_bytes);

        let crc = crc32fast::hash(data);
        let mtime = DosDateTime::from(std::time::SystemTime::now());
        let local_header = LocalFileHeader::new(data_size_u32, crc, mtime, path_size as u16);

        self.mmap.extend(local_header.as_bytes())?;
        self.mmap.extend(&padded_path)?;
        self.mmap.extend(data)?;

        let mut remaining = padding;
        while remaining > 0 {
            let chunk = remaining.min(ZERO_PAD.len());
            self.mmap.extend(&ZERO_PAD[..chunk])?;
            remaining -= chunk;
        }

        self.write_offset += aligned_size;

        let cd_header = CentralDirectoryHeader::new(
            data_size_u32,
            crc,
            mtime,
            local_offset_u32,
            mode,
            path_size as u16,
        );

        self.entries.push(CdEntry {
            header: cd_header,
            path: padded_path,
        });

        self.dirty = true;
        Ok(())
    }

    fn add_file(&mut self, src: impl AsRef<Path>, archive_path: &str) -> Result<(), BaleError> {
        let src = src.as_ref();
        let mut file = File::open(src)?;
        let metadata = file.metadata()?;

        let mut data = Vec::with_capacity(metadata.len() as usize);
        file.read_to_end(&mut data)?;

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

    fn delete(&mut self, path: &str) -> bool {
        let path_bytes = path.as_bytes();
        let path_size = self.path_size();

        if path_bytes.len() > path_size {
            return false;
        }

        let initial_len = self.entries.len();
        self.entries.retain(|entry| {
            !(entry.path.starts_with(path_bytes)
                && entry.path[path_bytes.len()..].iter().all(|&b| b == 0))
        });

        let deleted = self.entries.len() < initial_len;
        if deleted {
            self.dirty = true;
        }
        deleted
    }

    fn sync(&mut self) -> Result<(), BaleError> {
        if !self.dirty {
            return Ok(());
        }

        let path_size = self.path_size();
        let cd_stride = CentralDirectoryHeader::stride(path_size);

        let entry_count = self.entries.len() as u64;
        let cd_size = self.entries.len() * cd_stride;
        let cd_offset = self.write_offset;
        let total_size = cd_offset + cd_size + BaleEocd::COMBINED_SIZE;

        self.mmap.reserve(cd_size + BaleEocd::COMBINED_SIZE)?;
        self.mmap.set_len(total_size)?;

        let bytes = self.mmap.as_bytes_mut();

        let mut offset = cd_offset;
        for entry in &self.entries {
            bytes[offset..offset + CentralDirectoryHeader::SIZE]
                .copy_from_slice(entry.header.as_bytes());
            bytes[offset + CentralDirectoryHeader::SIZE..offset + cd_stride]
                .copy_from_slice(&entry.path);
            offset += cd_stride;
        }

        let zip64_eocd_offset = offset;
        let trailer = Trailer::new(
            entry_count,
            cd_size as u64,
            cd_offset as u64,
            zip64_eocd_offset as u64,
            self.bale_eocd,
        );
        bytes[offset..offset + Trailer::SIZE].copy_from_slice(&trailer.to_bytes());

        self.mmap.sync()?;
        self.mmap.set_len(self.write_offset)?;

        self.dirty = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ArchiveReader, ArchiveWriter};
    use std::io::Write;
    use tempfile::{NamedTempFile, TempDir};

    // ------------------------------------------------------------------------
    // Reader tests
    // ------------------------------------------------------------------------

    /// Opening a file that is too small returns TooSmall error.
    #[test]
    fn too_small_file() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(&[0u8; 100]).unwrap();

        let result = ArchiveReader::open(file.path());
        assert!(matches!(result, Err(BaleError::TooSmall { .. })));
    }

    /// Opening a file with invalid signatures returns error.
    #[test]
    fn invalid_trailer_signature() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(&[0u8; 256]).unwrap();

        let result = ArchiveReader::open(file.path());
        assert!(matches!(result, Err(BaleError::InvalidSignature { .. })));
    }

    /// Opening a valid empty archive works.
    #[test]
    fn open_empty_archive() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&path).unwrap();
        assert_eq!(reader.entry_count(), 0);
        assert_eq!(reader.path_size(), 256);
        assert_eq!(reader.alignment(), 4096);
    }

    /// Reading entries from an archive with files.
    #[test]
    fn read_entries() {
        let dir = TempDir::new().unwrap();
        let archive_path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&archive_path).unwrap();
            writer
                .add_entry("hello.txt", b"Hello, World!", 0o644)
                .unwrap();
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&archive_path).unwrap();
        assert_eq!(reader.entry_count(), 1);

        let entries: Vec<_> = reader.iter_entries().collect();
        assert_eq!(entries.len(), 1);
        let (header, path) = entries[0];
        assert!(path.starts_with(b"hello.txt"));
        assert_eq!(header.uncompressed_size.get(), 13);

        let data = reader.read_data(header).unwrap();
        assert_eq!(data, b"Hello, World!");
    }

    /// find_entry returns the entry for an existing path.
    #[test]
    fn find_existing_entry() {
        let dir = TempDir::new().unwrap();
        let archive_path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&archive_path).unwrap();
            writer.add_entry("a.txt", b"aaa", 0o644).unwrap();
            writer.add_entry("b.txt", b"bbbbb", 0o644).unwrap();
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&archive_path).unwrap();
        let entry_a = reader.find_entry("a.txt").unwrap();
        assert_eq!(entry_a.uncompressed_size.get(), 3);

        let entry_b = reader.find_entry("b.txt").unwrap();
        assert_eq!(entry_b.uncompressed_size.get(), 5);

        assert!(reader.find_entry("c.txt").is_none());
    }

    // ------------------------------------------------------------------------
    // Writer tests
    // ------------------------------------------------------------------------

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
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer
                .add_entry("hello.txt", b"Hello, World!", 0o644)
                .unwrap();
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&path).unwrap();
        assert_eq!(reader.entry_count(), 1);

        let entry = reader.find_entry("hello.txt").unwrap();
        let data = reader.read_data(entry).unwrap();
        assert_eq!(data, b"Hello, World!");
    }

    /// Adding multiple entries works.
    #[test]
    fn add_multiple_entries() {
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
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("first.txt", b"first", 0o644).unwrap();
            writer.sync().unwrap();
        }

        {
            let mut writer = ArchiveWriter::open(&path).unwrap();
            assert_eq!(writer.entry_count(), 1);
            writer.add_entry("second.txt", b"second", 0o644).unwrap();
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&path).unwrap();
        assert_eq!(reader.entry_count(), 2);
        assert!(reader.find_entry("first.txt").is_some());
        assert!(reader.find_entry("second.txt").is_some());
    }

    /// Shadow duplicates: new entry shadows old.
    #[test]
    fn shadow_duplicates() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("file.txt", b"original", 0o644).unwrap();
            writer.add_entry("file.txt", b"updated", 0o644).unwrap();
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&path).unwrap();
        assert_eq!(reader.entry_count(), 2);
        let entry = reader.find_entry("file.txt").unwrap();
        let data = reader.read_data(entry).unwrap();
        assert_eq!(data, b"updated");
    }

    /// add_file works with filesystem files.
    #[test]
    fn add_file_from_filesystem() {
        let dir = TempDir::new().unwrap();
        let archive_path = dir.path().join("test.bale");
        let src_path = dir.path().join("source.txt");

        {
            let mut f = File::create(&src_path).unwrap();
            f.write_all(b"file contents").unwrap();
        }

        {
            let mut writer = ArchiveWriter::create(&archive_path).unwrap();
            writer.add_file(&src_path, "source.txt").unwrap();
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&archive_path).unwrap();
        let entry = reader.find_entry("source.txt").unwrap();
        let data = reader.read_data(entry).unwrap();
        assert_eq!(data, b"file contents");
    }

    /// Adding entries after sync works correctly (same writer instance).
    #[test]
    fn add_after_sync() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();

            writer.add_entry("first.txt", b"first", 0o644).unwrap();
            writer.sync().unwrap();

            writer.add_entry("second.txt", b"second", 0o644).unwrap();
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&path).unwrap();
        assert_eq!(reader.entry_count(), 2);

        let first = reader.find_entry("first.txt").unwrap();
        assert_eq!(reader.read_data(first).unwrap(), b"first");

        let second = reader.find_entry("second.txt").unwrap();
        assert_eq!(reader.read_data(second).unwrap(), b"second");
    }

    // ------------------------------------------------------------------------
    // Writer can also read (ArchiveRead trait)
    // ------------------------------------------------------------------------

    /// Writer implements ArchiveRead trait.
    #[test]
    fn writer_can_read() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        let mut writer = ArchiveWriter::create(&path).unwrap();
        writer.add_entry("test.txt", b"test data", 0o644).unwrap();

        // Use ArchiveRead methods on writer.
        assert_eq!(writer.entry_count(), 1);
        let entry = writer.find_entry("test.txt").unwrap();
        let data = writer.read_data(entry).unwrap();
        assert_eq!(data, b"test data");
    }
}
