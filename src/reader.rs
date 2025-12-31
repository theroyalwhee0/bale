//! Zero-copy archive reader using memory-mapped I/O.

use crate::{BaleEocd, BaleError, CentralDirectoryHeader, Eocd, LocalFileHeader, MappedArchive};
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
