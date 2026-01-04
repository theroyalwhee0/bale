//! Read-only archive implementation.

use super::{Archive, ArchiveRead};
use crate::central_dir::parse_cd_entries;
use crate::{
    ArchivePath, BaleEocd, BaleError, CentralDirectoryHeader, Eocd, LocalFileHeader, MappedArchive,
    Trailer, Zip64Eocd,
};
use std::collections::HashSet;
use std::path::Path;

impl Archive<MappedArchive> {
    /// Opens an archive for reading.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file cannot be opened or memory-mapped
    /// - The archive is too small to contain a valid trailer
    /// - Any trailer signature is invalid (ZIP64 EOCD, Locator, EOCD, or BaleEocd)
    pub fn open(path: impl AsRef<Path>) -> Result<Self, BaleError> {
        let mmap = MappedArchive::open(path)?;
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
            write_offset: 0,
            dirty: false,
        })
    }

    /// Returns a reference to the parsed trailer.
    ///
    /// Note: This reconstructs the trailer from stored components. For most
    /// uses, prefer the individual accessor methods like [`bale_eocd()`](Self::bale_eocd).
    #[must_use]
    pub fn trailer(&self) -> Trailer {
        // Reconstruct trailer from stored data.
        // CD offset is computed from entries.
        let cd_offset = self.compute_cd_offset();
        let cd_size = self.entries.len() * CentralDirectoryHeader::stride(self.path_size());
        let zip64_eocd_offset = cd_offset + cd_size;

        Trailer::new(
            self.entries.len() as u64,
            cd_size as u64,
            cd_offset as u64,
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

    /// Computes the CD offset from entries.
    fn compute_cd_offset(&self) -> usize {
        if self.entries.is_empty() {
            return 0;
        }

        // Find the highest local_header_offset + entry size.
        let path_size = self.path_size();
        let local_stride = LocalFileHeader::stride(path_size);
        let alignment = self.alignment() as usize;

        let mut max_end: usize = 0;
        for entry in &self.entries {
            let local_offset = entry.header.local_header_offset.get() as usize;
            let data_size = entry.header.uncompressed_size.get() as usize;
            let unaligned_size = local_stride + data_size;
            let aligned_size = unaligned_size.div_ceil(alignment) * alignment;
            let entry_end = local_offset + aligned_size;
            max_end = max_end.max(entry_end);
        }

        max_end
    }
}

impl ArchiveRead for Archive<MappedArchive> {
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

        // Trim null padding for the ArchivePath.
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

    fn find_duplicates(&self) -> Vec<ArchivePath<'static>> {
        let mut seen: HashSet<&[u8]> = HashSet::new();
        let mut duplicate_set: HashSet<&[u8]> = HashSet::new();

        for entry in &self.entries {
            if !seen.insert(&entry.path) {
                duplicate_set.insert(&entry.path);
            }
        }

        duplicate_set
            .into_iter()
            .map(|path_bytes| {
                // Trim null padding.
                let end = path_bytes
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(path_bytes.len());
                // Create owned ArchivePath from trimmed bytes.
                ArchivePath::from(path_bytes[..end].to_vec())
            })
            .collect()
    }

    fn has_orphaned_data(&self) -> bool {
        if self.entries.is_empty() {
            return false;
        }

        // Sort entries by local_header_offset.
        let mut sorted_entries: Vec<_> = self.entries.iter().collect();
        sorted_entries.sort_by_key(|e| e.header.local_header_offset.get());

        let path_size = self.path_size();
        let alignment = self.alignment() as usize;
        let local_header_stride = LocalFileHeader::stride(path_size);
        let cd_offset = self.compute_cd_offset();

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

        expected_offset != cd_offset
    }
}
