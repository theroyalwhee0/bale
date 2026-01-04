//! Read-only archive implementation.

use super::{Archive, ArchiveRead, DirEntry, Entry, FileEntry, SymlinkEntry};
use crate::central_dir::parse_cd_entries;
use crate::{
    ArchivePath, BaleEocd, BaleError, CentralDirectoryHeader, EntryKind, Eocd, LocalFileHeader,
    MappedArchive, Trailer, Zip64Eocd,
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
        self.find_entry_with_path(path).map(|(h, _)| h)
    }

    fn find_entry_with_path(&self, path: &str) -> Option<(&CentralDirectoryHeader, &[u8])> {
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
                // Trim null padding from the stored path.
                let end = entry
                    .path
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(entry.path.len());
                result = Some((&entry.header, &entry.path[..end]));
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

    fn file(&self, path: impl AsRef<str>) -> Result<FileEntry<'_>, BaleError> {
        let path_str = path.as_ref();
        let (header, path_bytes) = self
            .find_entry_with_path(path_str)
            .ok_or_else(|| BaleError::EntryNotFound(path_str.to_string()))?;

        if header.kind() != EntryKind::File {
            return Err(BaleError::NotAFile(path_str.to_string()));
        }

        let data = self.read_data(header)?;
        let archive_path = ArchivePath::from_bytes(path_bytes);

        Ok(FileEntry {
            header,
            path: archive_path,
            data,
        })
    }

    fn folder(&self, path: impl AsRef<str>) -> Result<DirEntry<'_>, BaleError> {
        let path_str = path.as_ref();

        // Try with and without trailing slash.
        let found = self.find_entry_with_path(path_str).or_else(|| {
            if path_str.ends_with('/') {
                self.find_entry_with_path(path_str.trim_end_matches('/'))
            } else {
                self.find_entry_with_path(&format!("{path_str}/"))
            }
        });

        let (header, path_bytes) =
            found.ok_or_else(|| BaleError::EntryNotFound(path_str.to_string()))?;

        if header.kind() != EntryKind::Directory {
            return Err(BaleError::NotADirectory(path_str.to_string()));
        }

        // Trim trailing slash for the ArchivePath.
        let trimmed = if path_bytes.ends_with(b"/") {
            &path_bytes[..path_bytes.len() - 1]
        } else {
            path_bytes
        };
        let archive_path = ArchivePath::from_bytes(trimmed);

        Ok(DirEntry {
            header,
            path: archive_path,
        })
    }

    fn symlink(&self, path: impl AsRef<str>) -> Result<SymlinkEntry<'_>, BaleError> {
        let path_str = path.as_ref();
        let (header, path_bytes) = self
            .find_entry_with_path(path_str)
            .ok_or_else(|| BaleError::EntryNotFound(path_str.to_string()))?;

        if header.kind() != EntryKind::Symlink {
            return Err(BaleError::NotASymlink(path_str.to_string()));
        }

        let target = self.read_data(header)?;
        let archive_path = ArchivePath::from_bytes(path_bytes);

        Ok(SymlinkEntry {
            header,
            path: archive_path,
            target,
        })
    }

    fn entry(&self, path: impl AsRef<str>) -> Result<Entry<'_>, BaleError> {
        let path_str = path.as_ref();

        // Try direct lookup first.
        let found = self.find_entry_with_path(path_str).or_else(|| {
            // For directories, also try with/without trailing slash.
            if path_str.ends_with('/') {
                self.find_entry_with_path(path_str.trim_end_matches('/'))
            } else {
                self.find_entry_with_path(&format!("{path_str}/"))
            }
        });

        let (header, path_bytes) =
            found.ok_or_else(|| BaleError::EntryNotFound(path_str.to_string()))?;

        match header.kind() {
            EntryKind::File => {
                let data = self.read_data(header)?;
                let archive_path = ArchivePath::from_bytes(path_bytes);
                Ok(Entry::File(FileEntry {
                    header,
                    path: archive_path,
                    data,
                }))
            }
            EntryKind::Directory => {
                // Trim trailing slash for the ArchivePath.
                let trimmed = if path_bytes.ends_with(b"/") {
                    &path_bytes[..path_bytes.len() - 1]
                } else {
                    path_bytes
                };
                let archive_path = ArchivePath::from_bytes(trimmed);
                Ok(Entry::Directory(DirEntry {
                    header,
                    path: archive_path,
                }))
            }
            EntryKind::Symlink => {
                let target = self.read_data(header)?;
                let archive_path = ArchivePath::from_bytes(path_bytes);
                Ok(Entry::Symlink(SymlinkEntry {
                    header,
                    path: archive_path,
                    target,
                }))
            }
            EntryKind::Other(_) => {
                // Treat unknown types as files for now.
                let data = self.read_data(header)?;
                let archive_path = ArchivePath::from_bytes(path_bytes);
                Ok(Entry::File(FileEntry {
                    header,
                    path: archive_path,
                    data,
                }))
            }
        }
    }
}
