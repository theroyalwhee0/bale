//! Read-write archive implementation.

use super::{Archive, ArchiveRead, ArchiveWrite, DirEntry, Entry, FileEntry, SymlinkEntry};
use crate::central_dir::{BaleExtra, CdEntry, parse_cd_entries};
use crate::{
    ArchivePath, BaleEocd, BaleError, CentralDirectoryHeader, DosDateTime, EntryKind, Eocd,
    LocalFileHeader, MappedArchiveMut, Trailer, Zip64Eocd,
};
use nix::sys::stat::SFlag;
use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use zerocopy::IntoBytes;

/// Default permission bits for regular files (rw-r--r--).
/// Used on non-Unix platforms where file permissions aren't available.
#[cfg_attr(unix, allow(dead_code))]
const DEFAULT_FILE_PERM: u32 = 0o644;

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
        let mut mmap = MappedArchiveMut::open(path)?;
        let bytes = mmap.as_bytes();

        // Parse and validate trailer.
        let trailer = Trailer::from_archive_bytes(bytes)?;
        let bale_eocd = trailer.bale_eocd;
        let path_size = trailer.path_size() as usize;
        let cd_offset = trailer.cd_offset() as usize;
        let entry_count = trailer.entry_count() as usize;

        // Parse Central Directory entries.
        let entries = parse_cd_entries(bytes, cd_offset, entry_count, path_size)?;

        // Set mmap length to CD offset so new entries are written where the old
        // CD was. This ensures add_entry()'s local_offset matches where data is
        // actually written via mmap.extend().
        mmap.set_len(cd_offset)?;

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
        self.find_entry_with_path(path).map(|(h, _, _)| h)
    }

    fn find_entry_with_path(&self, path: &str) -> Option<(&CentralDirectoryHeader, &[u8], u32)> {
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
                result = Some((&entry.header, &entry.path[..end], entry.id));
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

    fn file(&self, path: impl AsRef<str>) -> Result<FileEntry<'_>, BaleError> {
        let path_str = path.as_ref();
        let (header, path_bytes, id) = self
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
            id,
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

        let (header, path_bytes, id) =
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
            id,
        })
    }

    fn symlink(&self, path: impl AsRef<str>) -> Result<SymlinkEntry<'_>, BaleError> {
        let path_str = path.as_ref();
        let (header, path_bytes, id) = self
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
            id,
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

        let (header, path_bytes, id) =
            found.ok_or_else(|| BaleError::EntryNotFound(path_str.to_string()))?;

        match header.kind() {
            EntryKind::File => {
                let data = self.read_data(header)?;
                let archive_path = ArchivePath::from_bytes(path_bytes);
                Ok(Entry::File(FileEntry {
                    header,
                    path: archive_path,
                    data,
                    id,
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
                    id,
                }))
            }
            EntryKind::Symlink => {
                let target = self.read_data(header)?;
                let archive_path = ArchivePath::from_bytes(path_bytes);
                Ok(Entry::Symlink(SymlinkEntry {
                    header,
                    path: archive_path,
                    target,
                    id,
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
                    id,
                }))
            }
        }
    }

    fn find_by_id(&self, id: u32) -> Option<Entry<'_>> {
        // Linear scan to find entry with matching ID.
        for entry in &self.entries {
            if entry.id == id {
                // Trim null padding from the stored path.
                let end = entry
                    .path
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(entry.path.len());
                let path_bytes = &entry.path[..end];
                let archive_path = ArchivePath::from_bytes(path_bytes);

                return match entry.header.kind() {
                    EntryKind::File | EntryKind::Other(_) => {
                        let data = self.read_data(&entry.header).ok()?;
                        Some(Entry::File(FileEntry {
                            header: &entry.header,
                            path: archive_path,
                            data,
                            id,
                        }))
                    }
                    EntryKind::Directory => {
                        // Trim trailing slash for the ArchivePath.
                        let trimmed = if path_bytes.ends_with(b"/") {
                            ArchivePath::from_bytes(&path_bytes[..path_bytes.len() - 1])
                        } else {
                            archive_path
                        };
                        Some(Entry::Directory(DirEntry {
                            header: &entry.header,
                            path: trimmed,
                            id,
                        }))
                    }
                    EntryKind::Symlink => {
                        let target = self.read_data(&entry.header).ok()?;
                        Some(Entry::Symlink(SymlinkEntry {
                            header: &entry.header,
                            path: archive_path,
                            target,
                            id,
                        }))
                    }
                };
            }
        }
        None
    }
}

impl ArchiveWrite for Archive<MappedArchiveMut> {
    fn add_entry(&mut self, path: &str, data: &[u8], mode: u32) -> Result<(), BaleError> {
        self.add_entry_with_mtime(path, data, mode, None)
    }

    fn add_entry_with_mtime(
        &mut self,
        path: &str,
        data: &[u8],
        mode: u32,
        mtime: Option<std::time::SystemTime>,
    ) -> Result<(), BaleError> {
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
        let dos_mtime = DosDateTime::from(mtime.unwrap_or_else(std::time::SystemTime::now));
        let local_header = LocalFileHeader::new(data_size_u32, crc, dos_mtime, path_size as u16);

        // Assign next available ID and increment counter.
        let id = self.bale_eocd.next_id();
        self.bale_eocd.set_next_id(id.saturating_add(1));
        let extra = BaleExtra::new(id);

        self.mmap.extend(local_header.as_bytes())?;
        self.mmap.extend(&padded_path)?;
        self.mmap.extend(extra.as_bytes())?;
        self.mmap.extend(data)?;

        if padding > 0 {
            self.mmap.set_len(self.mmap.len() + padding)?;
        }

        self.write_offset += aligned_size;

        let cd_header = CentralDirectoryHeader::new(
            data_size_u32,
            crc,
            dos_mtime,
            local_offset_u32,
            mode,
            path_size as u16,
        );

        self.entries.push(CdEntry {
            header: cd_header,
            path: padded_path,
            id,
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
                SFlag::S_IFREG.bits() | DEFAULT_FILE_PERM
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
            log::trace!("ArchiveWriter::sync: skipped (not dirty)");
            return Ok(());
        }

        let path_size = self.path_size();
        let cd_stride = CentralDirectoryHeader::stride(path_size);

        let entry_count = self.entries.len() as u64;
        let cd_size = self.entries.len() * cd_stride;
        let cd_offset = self.write_offset;
        let total_size = cd_offset + cd_size + BaleEocd::COMBINED_SIZE;

        log::trace!(
            "ArchiveWriter::sync: entries={}, write_offset={}, cd_size={}, total_size={}",
            entry_count,
            cd_offset,
            cd_size,
            total_size
        );

        self.mmap.reserve(cd_size + BaleEocd::COMBINED_SIZE)?;
        self.mmap.set_len(total_size)?;

        let bytes = self.mmap.as_bytes_mut();
        let extra_size = CentralDirectoryHeader::EXTRA_SIZE as usize;

        let mut offset = cd_offset;
        for entry in &self.entries {
            // Write header.
            bytes[offset..offset + CentralDirectoryHeader::SIZE]
                .copy_from_slice(entry.header.as_bytes());
            // Write path.
            let path_end = offset + CentralDirectoryHeader::SIZE + path_size;
            bytes[offset + CentralDirectoryHeader::SIZE..path_end].copy_from_slice(&entry.path);
            // Write extra field with ID.
            let extra = BaleExtra::new(entry.id);
            bytes[path_end..path_end + extra_size].copy_from_slice(extra.as_bytes());
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

        // Sync truncates file to len (which is total_size at this point).
        // This is important when entries are deleted - the file must shrink
        // so the new trailer is at the end.
        self.mmap.sync()?;
        // Reset len to write_offset for subsequent add operations.
        self.mmap.set_len(self.write_offset)?;

        self.dirty = false;
        Ok(())
    }

    fn add_folder(&mut self, path: impl AsRef<str>, mode: u32) -> Result<(), BaleError> {
        let path_str = path.as_ref();

        // Ensure directory mode bits are set.
        let mode = if mode & SFlag::S_IFMT.bits() == 0 {
            // No file type bits set, add directory bits.
            mode | SFlag::S_IFDIR.bits()
        } else {
            mode
        };

        // Directories have empty data and a trailing slash in their path.
        let dir_path = if path_str.ends_with('/') {
            path_str.to_string()
        } else {
            format!("{path_str}/")
        };

        self.add_entry(&dir_path, &[], mode)
    }

    fn add_symlink(
        &mut self,
        path: impl AsRef<str>,
        target: impl AsRef<str>,
        mode: u32,
    ) -> Result<(), BaleError> {
        let path_str = path.as_ref();
        let target_str = target.as_ref();

        // Ensure symlink mode bits are set.
        let mode = if mode & SFlag::S_IFMT.bits() == 0 {
            // No file type bits set, add symlink bits.
            mode | SFlag::S_IFLNK.bits()
        } else {
            mode
        };

        // Symlink target is stored as file data.
        self.add_entry(path_str, target_str.as_bytes(), mode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ArchiveReader, ArchiveWriter, Entry};
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

    /// Delete from reopened archive truncates file correctly.
    ///
    /// Regression test: after delete + sync, the file must be truncated so
    /// the new trailer is at the end. Otherwise the old trailer is read.
    #[test]
    fn delete_from_reopened_archive_truncates() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        // Create archive with 2 entries.
        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("a.txt", b"aaa", 0o644).unwrap();
            writer.add_entry("b.txt", b"bbb", 0o644).unwrap();
            writer.sync().unwrap();
        }

        let size_before = std::fs::metadata(&path).unwrap().len();

        // Reopen and delete one entry.
        {
            let mut writer = ArchiveWriter::open(&path).unwrap();
            assert_eq!(writer.entry_count(), 2);
            assert!(writer.delete("a.txt"));
            writer.sync().unwrap();
        }

        let size_after = std::fs::metadata(&path).unwrap().len();

        // File should be smaller (one fewer CD entry).
        assert!(
            size_after < size_before,
            "file should shrink after delete: before={size_before}, after={size_after}"
        );

        // Reopen and verify only 1 entry.
        let reader = ArchiveReader::open(&path).unwrap();
        assert_eq!(reader.entry_count(), 1);
        assert!(reader.find_entry("a.txt").is_none());
        assert!(reader.find_entry("b.txt").is_some());

        // Verify data is still readable.
        let entry = reader.find_entry("b.txt").unwrap();
        assert_eq!(reader.read_data(entry).unwrap(), b"bbb");
    }

    /// Opening an existing archive preserves entries and data offsets.
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

        // Verify data is readable at correct offsets (not just that entries exist).
        let first = reader.find_entry("first.txt").unwrap();
        assert_eq!(reader.read_data(first).unwrap(), b"first");

        let second = reader.find_entry("second.txt").unwrap();
        assert_eq!(reader.read_data(second).unwrap(), b"second");
    }

    /// Adding to an empty archive writes data at correct offset.
    ///
    /// Regression test for bug where open() didn't reset mmap.len to cd_offset,
    /// causing data to be written at the wrong location.
    #[test]
    fn add_to_empty_archive() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        // Create empty archive (like `bale touch`).
        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.sync().unwrap();
        }

        // Add file to empty archive (like `bale add`).
        {
            let mut writer = ArchiveWriter::open(&path).unwrap();
            assert_eq!(writer.entry_count(), 0);
            writer
                .add_entry("hello.txt", b"Hello, World!", 0o644)
                .unwrap();
            writer.sync().unwrap();
        }

        // Verify data is readable and CRC is correct.
        let reader = ArchiveReader::open(&path).unwrap();
        assert_eq!(reader.entry_count(), 1);

        let entry = reader.find_entry("hello.txt").unwrap();
        assert_eq!(reader.read_data(entry).unwrap(), b"Hello, World!");
        reader.verify_crc(entry).unwrap();
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

    // ------------------------------------------------------------------------
    // Entry type tests (FileEntry, DirEntry, SymlinkEntry, Entry)
    // ------------------------------------------------------------------------

    /// FileEntry provides access to file metadata and data.
    #[test]
    fn file_entry_accessors() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("test.txt", b"hello", 0o755).unwrap();
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&path).unwrap();
        let file = reader.file("test.txt").unwrap();

        assert_eq!(file.path().as_str(), Some("test.txt"));
        assert_eq!(file.data(), b"hello");
        assert_eq!(file.size(), 5);
        assert_eq!(file.mode(), 0o755);
        assert_eq!(file.crc32(), crc32fast::hash(b"hello"));
        assert!(file.id() > 0);
        assert!(file.header().signature.get() != 0);
        // mtime() returns a DosDateTime.
        let _ = file.mtime();
    }

    /// DirEntry provides access to directory metadata.
    #[test]
    fn dir_entry_accessors() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_folder("mydir", 0o755).unwrap();
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&path).unwrap();
        let folder = reader.folder("mydir").unwrap();

        assert_eq!(folder.path().as_str(), Some("mydir"));
        assert_eq!(folder.mode() & 0o777, 0o755);
        assert!(folder.id() > 0);
        assert!(folder.header().signature.get() != 0);
        let _ = folder.mtime();
    }

    /// SymlinkEntry provides access to symlink metadata and target.
    #[test]
    fn symlink_entry_accessors() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_symlink("link", "target.txt", 0o777).unwrap();
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&path).unwrap();
        let symlink = reader.symlink("link").unwrap();

        assert_eq!(symlink.path().as_str(), Some("link"));
        assert_eq!(symlink.target(), Some("target.txt"));
        assert_eq!(symlink.target_bytes(), b"target.txt");
        assert_eq!(symlink.mode() & 0o777, 0o777);
        assert!(symlink.id() > 0);
        assert!(symlink.header().signature.get() != 0);
        let _ = symlink.mtime();
    }

    /// Entry enum provides type checking and conversion.
    #[test]
    fn entry_enum_type_checks() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("file.txt", b"data", 0o644).unwrap();
            writer.add_folder("dir", 0o755).unwrap();
            writer.add_symlink("link", "file.txt", 0o777).unwrap();
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&path).unwrap();

        // Test file entry.
        let file_entry = reader.entry("file.txt").unwrap();
        assert!(file_entry.is_file());
        assert!(!file_entry.is_directory());
        assert!(!file_entry.is_symlink());
        assert!(file_entry.as_file().is_some());
        assert!(file_entry.as_directory().is_none());
        assert!(file_entry.as_symlink().is_none());

        // Test directory entry.
        let dir_entry = reader.entry("dir").unwrap();
        assert!(!dir_entry.is_file());
        assert!(dir_entry.is_directory());
        assert!(!dir_entry.is_symlink());
        assert!(dir_entry.as_file().is_none());
        assert!(dir_entry.as_directory().is_some());
        assert!(dir_entry.as_symlink().is_none());

        // Test symlink entry.
        let link_entry = reader.entry("link").unwrap();
        assert!(!link_entry.is_file());
        assert!(!link_entry.is_directory());
        assert!(link_entry.is_symlink());
        assert!(link_entry.as_file().is_none());
        assert!(link_entry.as_directory().is_none());
        assert!(link_entry.as_symlink().is_some());
    }

    /// Entry enum into_* conversions consume the entry.
    #[test]
    fn entry_enum_into_conversions() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("file.txt", b"data", 0o644).unwrap();
            writer.add_folder("dir", 0o755).unwrap();
            writer.add_symlink("link", "target", 0o777).unwrap();
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&path).unwrap();

        // into_file on file succeeds.
        let entry = reader.entry("file.txt").unwrap();
        assert!(entry.into_file().is_some());

        // into_directory on dir succeeds.
        let entry = reader.entry("dir").unwrap();
        assert!(entry.into_directory().is_some());

        // into_symlink on link succeeds.
        let entry = reader.entry("link").unwrap();
        assert!(entry.into_symlink().is_some());

        // into_file on non-file returns None.
        let entry = reader.entry("dir").unwrap();
        assert!(entry.into_file().is_none());
    }

    /// Entry From implementations work.
    #[test]
    fn entry_from_conversions() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("file.txt", b"data", 0o644).unwrap();
            writer.add_folder("dir", 0o755).unwrap();
            writer.add_symlink("link", "target", 0o777).unwrap();
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&path).unwrap();

        // From<FileEntry>.
        let file = reader.file("file.txt").unwrap();
        let entry: Entry = file.into();
        assert!(entry.is_file());

        // From<DirEntry>.
        let folder = reader.folder("dir").unwrap();
        let entry: Entry = folder.into();
        assert!(entry.is_directory());

        // From<SymlinkEntry>.
    }

    /// Adding a folder and syncing should truncate the file properly.
    #[test]
    fn add_folder_sync_truncates() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        // Create empty archive.
        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.sync().unwrap();
        }
        let size_after_touch = std::fs::metadata(&path).unwrap().len();

        // Reopen and add folder.
        {
            let mut writer = ArchiveWriter::open(&path).unwrap();
            writer.add_folder("testdir", 0o755).unwrap();
            writer.sync().unwrap();
        }
        let size_after_sync = std::fs::metadata(&path).unwrap().len();

        // File should be small (not 1MB from reserve()).
        assert!(
            size_after_sync < 10000,
            "file should be small after sync, got {} bytes",
            size_after_sync
        );
        assert!(
            size_after_sync > size_after_touch,
            "file should grow to include folder entry"
        );
    }
}
