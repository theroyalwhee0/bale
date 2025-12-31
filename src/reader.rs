//! Zero-copy archive reader using memory-mapped I/O.

use crate::{BaleEocd, BaleError, CentralDirectoryHeader, Eocd, LocalFileHeader, MappedArchive};
use std::collections::HashSet;
use std::path::Path;
use zerocopy::FromBytes;

/// A zero-copy reader for bale archives.
///
/// This struct provides read access to archive contents through memory-mapped
/// I/O, enabling zero-copy access to file data. The archive remains mapped
/// for the lifetime of this struct.
///
/// # Example
///
/// ```ignore
/// let reader = ArchiveReader::open("archive.bale")?;
/// for (header, path) in reader.iter_entries() {
///     let data = reader.read_data(header);
///     // process data...
/// }
/// ```
pub struct ArchiveReader {
    /// The memory-mapped archive.
    mmap: MappedArchive,
    /// Parsed EOCD from the trailer.
    eocd: Eocd,
    /// Parsed BaleEocd from the trailer.
    bale_eocd: BaleEocd,
}

impl ArchiveReader {
    /// Opens an archive for reading.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file cannot be opened or memory-mapped
    /// - The archive is too small to contain a valid trailer
    /// - The EOCD or BaleEocd signature is invalid
    pub fn open(path: impl AsRef<Path>) -> Result<Self, BaleError> {
        let mmap = MappedArchive::open(path)?;

        // Parse and validate trailer, extracting owned copies.
        let (eocd, bale_eocd) = Self::parse_trailer(&mmap)?;

        Ok(Self {
            mmap,
            eocd,
            bale_eocd,
        })
    }

    /// Parses and validates the trailer, returning owned copies of EOCD and BaleEocd.
    fn parse_trailer(mmap: &MappedArchive) -> Result<(Eocd, BaleEocd), BaleError> {
        let bytes = mmap.as_bytes();

        // Check minimum size.
        if bytes.len() < BaleEocd::COMBINED_SIZE {
            return Err(BaleError::TooSmall {
                size: bytes.len() as u64,
                minimum: BaleEocd::COMBINED_SIZE as u64,
            });
        }

        // Parse trailer from the last 256 bytes.
        let trailer_start = bytes.len() - BaleEocd::COMBINED_SIZE;
        let trailer = &bytes[trailer_start..];

        // Parse EOCD (first 22 bytes of trailer).
        let eocd = Eocd::ref_from_bytes(&trailer[..Eocd::SIZE])
            .map_err(|e| BaleError::Corrupted(format!("invalid EOCD: {e}")))?;

        // Validate EOCD signature.
        if eocd.signature.get() != Eocd::SIGNATURE {
            return Err(BaleError::InvalidSignature {
                expected: Eocd::SIGNATURE,
                found: eocd.signature.get(),
            });
        }

        // Parse BaleEocd (remaining 234 bytes).
        let bale_eocd = BaleEocd::ref_from_bytes(&trailer[Eocd::SIZE..])
            .map_err(|e| BaleError::Corrupted(format!("invalid BaleEocd: {e}")))?;

        // Validate BaleEocd magic.
        if !bale_eocd.is_valid() {
            return Err(BaleError::InvalidSignature {
                expected: BaleEocd::MAGIC,
                found: bale_eocd.magic.get(),
            });
        }

        Ok((*eocd, *bale_eocd))
    }

    /// Returns the number of entries in the archive.
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.eocd.cd_entries_total.get() as usize
    }

    /// Returns the configured path size for this archive.
    #[must_use]
    pub fn path_size(&self) -> usize {
        self.bale_eocd.path_size() as usize
    }

    /// Returns the configured alignment for this archive.
    #[must_use]
    pub fn alignment(&self) -> u32 {
        self.bale_eocd.alignment()
    }

    /// Returns an iterator over all Central Directory entries.
    ///
    /// Each item is a tuple of (header, path_bytes) where path_bytes is the
    /// null-padded path from the CD entry.
    pub fn iter_entries(&self) -> impl Iterator<Item = (&CentralDirectoryHeader, &[u8])> {
        let bytes = self.mmap.as_bytes();
        let cd_offset = self.eocd.cd_offset.get() as usize;
        let path_size = self.path_size();
        let stride = CentralDirectoryHeader::stride(path_size);
        let entry_count = self.entry_count();

        (0..entry_count).filter_map(move |i| {
            let entry_start = cd_offset + i * stride;
            let entry_end = entry_start + stride;
            if entry_end > bytes.len() {
                return None;
            }

            let entry_bytes = &bytes[entry_start..entry_end];
            let header = CentralDirectoryHeader::ref_from_bytes(
                &entry_bytes[..CentralDirectoryHeader::SIZE],
            )
            .ok()?;
            let path = &entry_bytes[CentralDirectoryHeader::SIZE..];

            Some((header, path))
        })
    }

    /// Finds an entry by path using linear scan.
    ///
    /// Returns the last matching entry (for shadowed duplicates).
    /// The path comparison is byte-exact against the null-padded path.
    #[must_use]
    pub fn find_entry(&self, path: &str) -> Option<&CentralDirectoryHeader> {
        let path_bytes = path.as_bytes();
        let path_size = self.path_size();

        // Path must fit within path_size.
        if path_bytes.len() > path_size {
            return None;
        }

        let mut result = None;

        for (header, stored_path) in self.iter_entries() {
            // Check if path matches (with null padding).
            if stored_path.starts_with(path_bytes)
                && stored_path[path_bytes.len()..].iter().all(|&b| b == 0)
            {
                result = Some(header);
            }
        }

        result
    }

    /// Returns a zero-copy slice of the file data for the given entry.
    ///
    /// # Errors
    ///
    /// Returns an error if the entry's offset or size is invalid.
    pub fn read_data(&self, entry: &CentralDirectoryHeader) -> Result<&[u8], BaleError> {
        let bytes = self.mmap.as_bytes();
        let local_offset = entry.local_header_offset.get() as usize;
        let path_size = self.path_size();
        let data_size = entry.uncompressed_size.get() as usize;

        // Calculate where the data starts (after LocalFileHeader + path).
        let data_start = local_offset + LocalFileHeader::stride(path_size);
        let data_end = data_start + data_size;

        if data_end > bytes.len() {
            return Err(BaleError::Corrupted(format!(
                "entry data extends beyond archive: offset={local_offset}, size={data_size}"
            )));
        }

        Ok(&bytes[data_start..data_end])
    }

    /// Returns a reference to the EOCD.
    #[must_use]
    pub fn eocd(&self) -> &Eocd {
        &self.eocd
    }

    /// Returns a reference to the BaleEocd.
    #[must_use]
    pub fn bale_eocd(&self) -> &BaleEocd {
        &self.bale_eocd
    }

    /// Returns CRC information for an entry: (computed, local_header, cd_header).
    ///
    /// This allows callers to determine the type of CRC mismatch:
    /// - If computed == local != cd: CD header is corrupted
    /// - If computed == cd != local: Local header is corrupted
    /// - If computed != local && computed != cd: Unknown corruption
    ///
    /// # Errors
    ///
    /// Returns an error if the entry data or local header cannot be read.
    pub fn crc_info(&self, entry: &CentralDirectoryHeader) -> Result<(u32, u32, u32), BaleError> {
        let bytes = self.mmap.as_bytes();
        let local_offset = entry.local_header_offset.get() as usize;

        // Read local file header.
        let local_end = local_offset + LocalFileHeader::SIZE;
        if local_end > bytes.len() {
            return Err(BaleError::Corrupted(format!(
                "local header extends beyond archive: offset={local_offset}"
            )));
        }
        let local_header = LocalFileHeader::ref_from_bytes(&bytes[local_offset..local_end])
            .map_err(|e| BaleError::Corrupted(format!("invalid local header: {e}")))?;

        let data = self.read_data(entry)?;
        let computed = crc32fast::hash(data);
        let local_crc = local_header.crc32.get();
        let cd_crc = entry.crc32.get();

        Ok((computed, local_crc, cd_crc))
    }

    /// Verifies the CRC-32 checksum for an entry.
    ///
    /// Reads the entry data and computes its CRC-32, comparing against the
    /// stored value in the Central Directory header.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The entry data cannot be read
    /// - The computed CRC does not match the stored CRC
    pub fn verify_crc(&self, entry: &CentralDirectoryHeader) -> Result<(), BaleError> {
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

    /// Checks if the Central Directory is sorted by path bytes.
    ///
    /// A sorted CD enables binary search for entry lookup. Archives created
    /// by `compact` are always sorted.
    #[must_use]
    pub fn is_sorted(&self) -> bool {
        let entries: Vec<_> = self.iter_entries().collect();
        entries.windows(2).all(|w| w[0].1 <= w[1].1)
    }

    /// Returns a list of duplicate paths in the archive.
    ///
    /// Duplicate paths occur when the same path appears multiple times in the
    /// Central Directory (shadowing). Returns the paths that have duplicates,
    /// not the total count of duplicates.
    #[must_use]
    pub fn find_duplicates(&self) -> Vec<String> {
        let mut seen: HashSet<&[u8]> = HashSet::new();
        let mut dup_set: HashSet<&[u8]> = HashSet::new();

        for (_header, path_bytes) in self.iter_entries() {
            if !seen.insert(path_bytes) {
                dup_set.insert(path_bytes);
            }
        }

        dup_set.into_iter().map(Self::path_to_string).collect()
    }

    /// Checks if the archive contains orphaned data.
    ///
    /// Orphaned data exists when there are gaps between entries or between
    /// the last entry and the Central Directory. This can occur after
    /// deletions or when entries are shadowed.
    #[must_use]
    pub fn has_orphaned_data(&self) -> bool {
        let entries: Vec<_> = self.iter_entries().collect();

        if entries.is_empty() {
            return false;
        }

        let path_size = self.path_size();
        let alignment = self.alignment() as usize;
        let local_header_stride = LocalFileHeader::stride(path_size);
        let cd_offset = self.eocd.cd_offset.get() as usize;

        let mut expected_offset: usize = 0;

        for (header, _path_bytes) in &entries {
            let local_offset = header.local_header_offset.get() as usize;
            let data_size = header.uncompressed_size.get() as usize;

            // Check if entry starts where expected.
            if local_offset != expected_offset {
                return true;
            }

            // Calculate next expected offset (aligned).
            let entry_size = local_header_stride + data_size;
            let aligned_size = entry_size.div_ceil(alignment) * alignment;
            expected_offset = local_offset + aligned_size;
        }

        // Check if CD starts right after the last entry.
        expected_offset != cd_offset
    }

    /// Converts a null-padded path to a string.
    fn path_to_string(path_bytes: &[u8]) -> String {
        let end = path_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(path_bytes.len());
        String::from_utf8_lossy(&path_bytes[..end]).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Opening a file that is too small returns TooSmall error.
    #[test]
    fn too_small_file() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::new().unwrap();
        file.write_all(&[0u8; 100]).unwrap(); // Less than 256 bytes

        let result = ArchiveReader::open(file.path());
        assert!(matches!(result, Err(BaleError::TooSmall { .. })));
    }

    /// Opening a file with invalid EOCD signature returns error.
    #[test]
    fn invalid_eocd_signature() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::new().unwrap();
        file.write_all(&[0u8; 256]).unwrap(); // All zeros, invalid signature

        let result = ArchiveReader::open(file.path());
        assert!(matches!(result, Err(BaleError::InvalidSignature { .. })));
    }

    /// Opening a valid empty archive works.
    #[test]
    fn open_empty_archive() {
        use crate::ArchiveWriter;
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        // Create empty archive using the writer (drop to release lock).
        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.sync().unwrap();
        }

        // Open with reader.
        let reader = ArchiveReader::open(&path).unwrap();
        assert_eq!(reader.entry_count(), 0);
        assert_eq!(reader.path_size(), 256);
        assert_eq!(reader.alignment(), 4096);
    }

    /// Reading entries from an archive with files.
    #[test]
    fn read_entries() {
        use crate::ArchiveWriter;
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let archive_path = dir.path().join("test.bale");

        // Create archive with one file (drop to release lock).
        {
            let mut writer = ArchiveWriter::create(&archive_path).unwrap();
            writer
                .add_entry("hello.txt", b"Hello, World!", 0o644)
                .unwrap();
            writer.sync().unwrap();
        }

        // Open with reader.
        let reader = ArchiveReader::open(&archive_path).unwrap();
        assert_eq!(reader.entry_count(), 1);

        // Check entry.
        let entries: Vec<_> = reader.iter_entries().collect();
        assert_eq!(entries.len(), 1);
        let (header, path) = entries[0];
        assert!(path.starts_with(b"hello.txt"));
        assert_eq!(header.uncompressed_size.get(), 13);

        // Read data.
        let data = reader.read_data(header).unwrap();
        assert_eq!(data, b"Hello, World!");
    }

    /// find_entry returns the entry for an existing path.
    #[test]
    fn find_existing_entry() {
        use crate::ArchiveWriter;
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let archive_path = dir.path().join("test.bale");

        // Create archive (drop to release lock).
        {
            let mut writer = ArchiveWriter::create(&archive_path).unwrap();
            writer.add_entry("a.txt", b"aaa", 0o644).unwrap();
            writer.add_entry("b.txt", b"bbbbb", 0o644).unwrap();
            writer.sync().unwrap();
        }

        // Open and find entries.
        let reader = ArchiveReader::open(&archive_path).unwrap();
        let entry_a = reader.find_entry("a.txt").unwrap();
        assert_eq!(entry_a.uncompressed_size.get(), 3);

        let entry_b = reader.find_entry("b.txt").unwrap();
        assert_eq!(entry_b.uncompressed_size.get(), 5);

        // Non-existent entry.
        assert!(reader.find_entry("c.txt").is_none());
    }
}
