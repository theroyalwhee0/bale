//! Read-write archive implementation.
//!
//! Implements the v1.0.0 archive writer that produces: file header, aligned
//! data blocks, entry table, directory table, and trailer. Uses in-memory
//! tables (`entry_rows`, `dir_entries`) to accumulate entries before flushing
//! to disk via [`sync()`](ArchiveWrite::sync).

use super::{Archive, ArchiveRead, ArchiveWrite, DirEntry, Entry, FileEntry, SymlinkEntry};
use crate::format::{Crc, DirectoryRow, EntryRow, FileHeader, Trailer};
use crate::{ArchivePath, BaleError, EntryKind, MappedArchiveMut};
use nix::sys::stat::SFlag;
use std::collections::HashSet;
use std::path::Path;
use std::time::SystemTime;
use zerocopy::byteorder::little_endian::{I64, U32, U64};
use zerocopy::{FromBytes, IntoBytes};

/// Converts a `SystemTime` to Unix epoch milliseconds.
///
/// Returns a negative value for times before the Unix epoch.
fn system_time_to_millis(time: SystemTime) -> i64 {
    match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d.as_millis() as i64,
        Err(e) => -(e.duration().as_millis() as i64),
    }
}

/// Returns the current time as Unix epoch milliseconds.
fn now_millis() -> i64 {
    system_time_to_millis(SystemTime::now())
}

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
    /// * `alignment` - Alignment for data blocks (must be power of 2)
    /// * `path_size` - Maximum path size (1-4096)
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
        let trailer = Trailer::new_with_options(alignment, path_size)?;
        let mut mmap = MappedArchiveMut::create(path)?;

        // Write the file header immediately.
        let file_header = FileHeader::new();
        mmap.extend(file_header.as_bytes())?;

        Ok(Self {
            mmap,
            trailer,
            write_offset: FileHeader::SIZE,
            dirty: true, // New archive needs initial sync to write trailer.
            entry_rows: Vec::new(),
            dir_entries: Vec::new(),
        })
    }

    /// Opens an existing archive for appending.
    ///
    /// Reads the trailer to determine table locations and configuration,
    /// then parses existing entry and directory tables into memory.
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
        let len = bytes.len() as u64;

        if len < Trailer::MIN_ARCHIVE_SIZE {
            return Err(BaleError::TooSmall {
                size: len,
                minimum: Trailer::MIN_ARCHIVE_SIZE,
            });
        }

        // Read and validate trailer from last 64 bytes.
        let trailer_bytes = &bytes[bytes.len() - Trailer::SIZE..];
        let trailer = *Trailer::ref_from_bytes(trailer_bytes)
            .map_err(|e| BaleError::Corrupted(format!("invalid trailer: {e}")))?;
        trailer.validated()?;

        // Parse existing entry table into memory.
        let entry_count = trailer.entry_count.get() as usize;
        let entry_rows = {
            let mut rows = Vec::with_capacity(entry_count);
            let offset = trailer.entry_table_offset.get() as usize;
            for i in 0..entry_count {
                let start = offset + i * EntryRow::SIZE;
                let end = start + EntryRow::SIZE;
                let row = *EntryRow::ref_from_bytes(&bytes[start..end])
                    .map_err(|e| BaleError::Corrupted(format!("invalid entry row: {e}")))?;
                rows.push(row);
            }
            rows
        };

        // Parse existing directory table into memory.
        let dir_count = trailer.directory_entry_count.get() as usize;
        let path_size = trailer.path_size();
        let stride = DirectoryRow::stride(path_size);
        let dir_entries = {
            let mut entries = Vec::with_capacity(dir_count);
            let offset = trailer.directory_table_offset.get() as usize;
            for i in 0..dir_count {
                let start = offset + i * stride;
                let row = DirectoryRow::from_bytes(&bytes[start..start + stride], path_size)?;
                let path_bytes = row.path_bytes_raw().to_vec();
                let entry_id = row.entry_id();
                entries.push((path_bytes, entry_id));
            }
            entries
        };

        // The write offset is where new data blocks should go — at the entry
        // table offset (since we'll rewrite tables on sync).
        let write_offset = trailer.entry_table_offset.get() as usize;

        Ok(Self {
            mmap,
            trailer,
            write_offset,
            dirty: false,
            entry_rows,
            dir_entries,
        })
    }

    /// Null-pads a path to the configured path_size.
    ///
    /// Returns the padded bytes or an error if the path is too long.
    fn pad_path(&self, path: &str) -> Result<Vec<u8>, BaleError> {
        let path_size = self.trailer.path_size() as usize;
        if path.len() > path_size {
            return Err(BaleError::PathTooLong {
                path: path.to_owned(),
                max: path_size,
            });
        }
        let mut buf = vec![0u8; path_size];
        buf[..path.len()].copy_from_slice(path.as_bytes());
        Ok(buf)
    }

    /// Writes a data block to the mmap at the next aligned offset.
    ///
    /// Returns `(data_offset, crc)` where `data_offset` is 0 for empty data.
    ///
    /// # Errors
    ///
    /// Returns an error if writing to the mmap fails.
    fn write_data_block(&mut self, data: &[u8]) -> Result<(u64, Crc), BaleError> {
        if data.is_empty() {
            return Ok((0, Crc::NONE));
        }

        // Pad to alignment boundary.
        let alignment = self.trailer.alignment()? as usize;
        let current = self.write_offset;
        let padding = (alignment - (current % alignment)) % alignment;
        if padding > 0 {
            self.mmap.set_len(current + padding)?;
            // set_len zero-fills the gap.
        }
        self.write_offset = current + padding;

        let data_offset = self.write_offset as u64;

        // Compute CRC before writing.
        let crc = Crc::compute(data);

        // Write the raw data (no header).
        self.mmap.set_len(self.write_offset)?;
        self.mmap.extend(data)?;
        self.write_offset += data.len();

        Ok((data_offset, crc))
    }

    /// Constructs an `Entry` from an in-memory entry row, path bytes, and ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the entry data cannot be read.
    fn make_entry<'a>(
        &'a self,
        row: &'a EntryRow,
        path_bytes: &'a [u8],
        id: u32,
    ) -> Result<Entry<'a>, BaleError> {
        let path = ArchivePath::from_null_padded_bytes(path_bytes);
        match row.kind() {
            EntryKind::File => {
                let data = self.read_data(row)?;
                Ok(Entry::File(FileEntry {
                    entry: row,
                    path,
                    data,
                    id,
                }))
            }
            EntryKind::Directory => Ok(Entry::Directory(DirEntry {
                entry: row,
                path,
                id,
            })),
            EntryKind::Symlink => {
                let target = self.read_data(row)?;
                Ok(Entry::Symlink(SymlinkEntry {
                    entry: row,
                    path,
                    target,
                    id,
                }))
            }
            EntryKind::Other(_) => Err(BaleError::Corrupted(format!(
                "unknown entry kind for mode {:#o}",
                row.mode.get()
            ))),
        }
    }

    /// Finds an entry row by ID using linear search on the in-memory table.
    fn find_entry_row_by_id(&self, id: u32) -> Option<&EntryRow> {
        self.entry_rows.iter().find(|r| r.entry_id.get() == id)
    }
}

impl ArchiveRead for Archive<MappedArchiveMut> {
    /// Returns the number of entries.
    fn entry_count(&self) -> usize {
        self.entry_rows.len()
    }

    /// Returns the configured path size.
    fn path_size(&self) -> usize {
        self.trailer.path_size() as usize
    }

    /// Returns the configured alignment.
    ///
    /// # Errors
    ///
    /// Returns an error if `alignment_power` is invalid.
    fn alignment(&self) -> Result<u32, BaleError> {
        self.trailer.alignment()
    }

    /// Returns the path at the given directory table index.
    fn get_path(&self, index: usize) -> Option<ArchivePath<'_>> {
        let (path_bytes, _) = self.dir_entries.get(index)?;
        Some(ArchivePath::from_null_padded_bytes(path_bytes))
    }

    /// Iterates over all entry rows with their paths.
    fn iter_entries(&self) -> impl Iterator<Item = (&EntryRow, &[u8])> {
        self.dir_entries.iter().filter_map(|(path_bytes, id)| {
            let row = self.find_entry_row_by_id(*id)?;
            Some((row, path_bytes.as_slice()))
        })
    }

    /// Finds an entry by path.
    fn find_entry(&self, path: &str) -> Option<&EntryRow> {
        let path_size = self.trailer.path_size() as usize;
        let padded = {
            let mut buf = vec![0u8; path_size];
            let len = path.len().min(path_size);
            buf[..len].copy_from_slice(&path.as_bytes()[..len]);
            buf
        };
        // Linear search the directory table for matching path.
        for (dir_path, id) in &self.dir_entries {
            if *dir_path == padded {
                return self.find_entry_row_by_id(*id);
            }
        }
        None
    }

    /// Finds an entry by path with path bytes and ID.
    fn find_entry_with_path(&self, path: &str) -> Option<(&EntryRow, &[u8], u32)> {
        let path_size = self.trailer.path_size() as usize;
        let padded = {
            let mut buf = vec![0u8; path_size];
            let len = path.len().min(path_size);
            buf[..len].copy_from_slice(&path.as_bytes()[..len]);
            buf
        };
        for (dir_path, id) in &self.dir_entries {
            if *dir_path == padded {
                let row = self.find_entry_row_by_id(*id)?;
                // Return trimmed path bytes (without null padding).
                let trimmed_len = dir_path
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(dir_path.len());
                return Some((row, &dir_path[..trimmed_len], *id));
            }
        }
        None
    }

    /// Reads data for the given entry row.
    ///
    /// # Errors
    ///
    /// Returns an error if the data offset or size is invalid.
    fn read_data(&self, entry: &EntryRow) -> Result<&[u8], BaleError> {
        let offset = entry.data_offset.get();
        if offset == 0 {
            return Ok(&[]);
        }
        let offset = offset as usize;
        let block_size = entry.block_size.get() as usize;
        let bytes = self.mmap.as_bytes();
        let data_end = offset + block_size;
        if data_end > bytes.len() {
            return Err(BaleError::Corrupted(format!(
                "data block at offset {offset} extends beyond archive (end {data_end} > len {})",
                bytes.len()
            )));
        }
        Ok(&bytes[offset..data_end])
    }

    /// Returns a reference to the trailer.
    fn trailer(&self) -> &Trailer {
        &self.trailer
    }

    /// Verifies the CRC-32C checksum for an entry.
    ///
    /// # Errors
    ///
    /// Returns an error if the CRC does not match.
    fn verify_crc(&self, entry: &EntryRow) -> Result<(), BaleError> {
        let Some(stored_crc) = entry.crc().get() else {
            // No CRC; nothing to verify (directory or empty entry).
            return Ok(());
        };
        let data = self.read_data(entry)?;
        let computed = Crc::compute(data);

        if stored_crc != computed.to_u32() {
            return Err(BaleError::Corrupted(format!(
                "CRC-32C mismatch: stored {stored_crc:#010x}, computed {:#010x}",
                computed.to_u32()
            )));
        }
        Ok(())
    }

    /// Verifies the metadata CRC-32C for the archive.
    ///
    /// For writers, the metadata CRC is computed during [`sync()`](ArchiveWrite::sync).
    /// This method recomputes and validates it against the on-disk trailer.
    ///
    /// # Errors
    ///
    /// Returns an error if the computed CRC does not match the stored CRC.
    fn verify_metadata_crc(&self) -> Result<(), BaleError> {
        let file_header = &self.mmap.as_bytes()[..FileHeader::SIZE];
        let entry_table = {
            let offset = self.trailer.entry_table_offset.get() as usize;
            let len = self.entry_rows.len() * EntryRow::SIZE;
            if len == 0 {
                &[] as &[u8]
            } else {
                &self.mmap.as_bytes()[offset..offset + len]
            }
        };
        let directory_table = {
            let count = self.trailer.directory_entry_count.get() as usize;
            if count == 0 {
                &[] as &[u8]
            } else {
                let offset = self.trailer.directory_table_offset.get() as usize;
                let stride = DirectoryRow::stride(self.trailer.path_size());
                let len = count * stride;
                &self.mmap.as_bytes()[offset..offset + len]
            }
        };
        let mut trailer_for_crc = self.trailer;
        trailer_for_crc.set_metadata_crc(Crc::NONE);
        let computed = Trailer::compute_metadata_crc(
            file_header,
            entry_table,
            directory_table,
            &trailer_for_crc,
        );
        let stored = self.trailer.metadata_crc();
        if stored != computed {
            let stored_val = stored.to_u32();
            let computed_val = computed.to_u32();
            return Err(BaleError::Corrupted(format!(
                "metadata CRC-32C mismatch: stored {stored_val:#010x}, computed {computed_val:#010x}"
            )));
        }
        Ok(())
    }

    /// Checks if the directory table is sorted.
    fn is_sorted(&self) -> bool {
        self.dir_entries.windows(2).all(|w| w[0].0 <= w[1].0)
    }

    /// Returns duplicate paths.
    fn find_duplicates(&self) -> Vec<ArchivePath<'static>> {
        if self.dir_entries.len() <= 1 {
            return Vec::new();
        }
        let mut duplicates = Vec::new();
        let mut seen = HashSet::new();
        for i in 1..self.dir_entries.len() {
            if self.dir_entries[i - 1].0 == self.dir_entries[i].0
                && seen.insert(self.dir_entries[i].0.clone())
            {
                let path = ArchivePath::from_null_padded_bytes(&self.dir_entries[i].0);
                duplicates.push(path.into_owned());
            }
        }
        duplicates
    }

    /// Checks for orphaned data blocks.
    ///
    /// # Errors
    ///
    /// Returns an error if the alignment is invalid.
    fn has_orphaned_data(&self) -> Result<bool, BaleError> {
        let referenced: HashSet<u64> = self
            .entry_rows
            .iter()
            .map(|row| row.data_offset.get())
            .filter(|&offset| offset != 0)
            .collect();

        let alignment = self.alignment()? as u64;
        let data_region_end = self.write_offset as u64;

        let mut offset = {
            let start = FileHeader::SIZE as u64;
            if alignment == 0 {
                return Ok(false);
            }
            start.div_ceil(alignment) * alignment
        };

        while offset < data_region_end {
            if !referenced.contains(&offset) {
                return Ok(true);
            }
            // Skip to next alignment boundary past this data block.
            let entry = self
                .entry_rows
                .iter()
                .find(|r| r.data_offset.get() == offset);
            let block_size = entry.map_or(0, |e| e.block_size.get());
            let next = if block_size > 0 {
                ((offset + block_size).div_ceil(alignment)) * alignment
            } else {
                offset + alignment
            };
            offset = next;
        }
        Ok(false)
    }

    /// Returns a file entry by path.
    ///
    /// # Errors
    ///
    /// Returns an error if not found or not a file.
    fn file(&self, path: impl AsRef<str>) -> Result<FileEntry<'_>, BaleError> {
        let path = path.as_ref();
        let (entry, path_bytes, id) = self
            .find_entry_with_path(path)
            .ok_or_else(|| BaleError::EntryNotFound(path.to_owned()))?;
        if entry.kind() != EntryKind::File {
            return Err(BaleError::NotAFile(path.to_owned()));
        }
        let data = self.read_data(entry)?;
        Ok(FileEntry {
            entry,
            path: ArchivePath::from_null_padded_bytes(path_bytes),
            data,
            id,
        })
    }

    /// Returns a directory entry by path.
    ///
    /// # Errors
    ///
    /// Returns an error if not found or not a directory.
    fn folder(&self, path: impl AsRef<str>) -> Result<DirEntry<'_>, BaleError> {
        let path = path.as_ref();
        let (entry, path_bytes, id) = self
            .find_entry_with_path(path)
            .ok_or_else(|| BaleError::EntryNotFound(path.to_owned()))?;
        if entry.kind() != EntryKind::Directory {
            return Err(BaleError::NotADirectory(path.to_owned()));
        }
        Ok(DirEntry {
            entry,
            path: ArchivePath::from_null_padded_bytes(path_bytes),
            id,
        })
    }

    /// Returns a symlink entry by path.
    ///
    /// # Errors
    ///
    /// Returns an error if not found or not a symlink.
    fn symlink(&self, path: impl AsRef<str>) -> Result<SymlinkEntry<'_>, BaleError> {
        let path = path.as_ref();
        let (entry, path_bytes, id) = self
            .find_entry_with_path(path)
            .ok_or_else(|| BaleError::EntryNotFound(path.to_owned()))?;
        if entry.kind() != EntryKind::Symlink {
            return Err(BaleError::NotASymlink(path.to_owned()));
        }
        let target = self.read_data(entry)?;
        Ok(SymlinkEntry {
            entry,
            path: ArchivePath::from_null_padded_bytes(path_bytes),
            target,
            id,
        })
    }

    /// Returns any entry by path.
    ///
    /// # Errors
    ///
    /// Returns an error if not found.
    fn entry(&self, path: impl AsRef<str>) -> Result<Entry<'_>, BaleError> {
        let path = path.as_ref();
        let (entry, path_bytes, id) = self
            .find_entry_with_path(path)
            .ok_or_else(|| BaleError::EntryNotFound(path.to_owned()))?;
        self.make_entry(entry, path_bytes, id)
    }

    /// Finds an entry by its stable ID.
    fn find_by_id(&self, id: u32) -> Option<Entry<'_>> {
        let entry_row = self.find_entry_row_by_id(id)?;
        // Linear scan directory table for this entry ID.
        for (path_bytes, dir_id) in &self.dir_entries {
            if *dir_id == id {
                let trimmed_len = path_bytes
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(path_bytes.len());
                return self
                    .make_entry(entry_row, &path_bytes[..trimmed_len], id)
                    .ok();
            }
        }
        None
    }
}

impl ArchiveWrite for Archive<MappedArchiveMut> {
    /// Adds an entry from raw data.
    ///
    /// # Errors
    ///
    /// Returns an error if the path is invalid or writing fails.
    fn add_entry(&mut self, path: &str, data: &[u8], mode: u32) -> Result<(), BaleError> {
        self.add_entry_with_mtime(path, data, mode, None)
    }

    /// Adds an entry with a specific modification time.
    ///
    /// # Errors
    ///
    /// Returns an error if the path is invalid or writing fails.
    fn add_entry_with_mtime(
        &mut self,
        path: &str,
        data: &[u8],
        mode: u32,
        mtime: Option<SystemTime>,
    ) -> Result<(), BaleError> {
        // Validate and normalize path.
        let normalized = ArchivePath::from_bytes(path.as_bytes()).normalize()?;
        let normalized_str = normalized
            .as_str()
            .ok_or_else(|| BaleError::InvalidPath(path.to_string()))?;

        // Check path fits within path_size.
        let padded = self.pad_path(normalized_str)?;

        // Assign entry ID, checking for overflow.
        let entry_id = self.trailer.next_id();
        if entry_id == u32::MAX {
            return Err(BaleError::ArchiveFull);
        }
        self.trailer.set_next_id(entry_id + 1);

        // Write data block (returns offset and CRC).
        let (data_offset, crc) = self.write_data_block(data)?;

        // Create entry row with timestamps.
        let now = now_millis();
        let modified_time = mtime.map_or(now, system_time_to_millis);
        let entry_row = EntryRow::new_file(
            entry_id,
            crc,
            data_offset,
            data.len() as u64,
            data.len() as u64,
            now,
            modified_time,
            mode,
        );

        // Add to in-memory tables.
        self.entry_rows.push(entry_row);
        self.dir_entries.push((padded, entry_id));
        self.dirty = true;

        Ok(())
    }

    /// Adds a file from the filesystem.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or writing fails.
    fn add_file(&mut self, src: impl AsRef<Path>, archive_path: &str) -> Result<(), BaleError> {
        let src = src.as_ref();
        let data = std::fs::read(src)?;
        let metadata = std::fs::metadata(src)?;

        // Extract mode from metadata.
        let mode = {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                metadata.permissions().mode()
            }
            #[cfg(not(unix))]
            {
                0o100644
            }
        };

        // Extract mtime.
        let mtime = metadata.modified().ok();

        self.add_entry_with_mtime(archive_path, &data, mode, mtime)
    }

    /// Deletes all entries matching a path.
    fn delete(&mut self, path: &str) -> bool {
        let path_size = self.trailer.path_size() as usize;
        let padded = {
            let mut buf = vec![0u8; path_size];
            let len = path.len().min(path_size);
            buf[..len].copy_from_slice(&path.as_bytes()[..len]);
            buf
        };

        // Collect entry IDs being removed.
        let removed_ids: HashSet<u32> = self
            .dir_entries
            .iter()
            .filter(|(p, _)| *p == padded)
            .map(|(_, id)| *id)
            .collect();

        if removed_ids.is_empty() {
            return false;
        }

        // Remove from both tables.
        self.dir_entries.retain(|(p, _)| *p != padded);
        self.entry_rows
            .retain(|r| !removed_ids.contains(&r.entry_id.get()));
        self.dirty = true;
        true
    }

    /// Creates a hard link to an existing entry.
    ///
    /// # Errors
    ///
    /// Returns an error if the target is not found, is a directory, the link
    /// path already exists, or the link path exceeds path_size.
    fn hard_link(&mut self, target: &str, link: &str) -> Result<(), BaleError> {
        // Find the target entry and its ID.
        let (entry_row, _, entry_id) = self
            .find_entry_with_path(target)
            .ok_or_else(|| BaleError::EntryNotFound(target.to_owned()))?;

        // Directories cannot be hard linked.
        if entry_row.kind() == EntryKind::Directory {
            return Err(BaleError::NotAFile(target.to_owned()));
        }

        // Normalize the link path.
        let normalized = ArchivePath::from_bytes(link.as_bytes()).normalize()?;
        let normalized_str = normalized
            .as_str()
            .ok_or_else(|| BaleError::InvalidPath(link.to_string()))?;

        // Verify link path doesn't already exist.
        if self.find_entry(normalized_str).is_some() {
            return Err(BaleError::PathExists(normalized_str.to_owned()));
        }

        // Pad and append the new directory entry.
        let padded = self.pad_path(normalized_str)?;
        self.dir_entries.push((padded, entry_id));
        self.dirty = true;

        Ok(())
    }

    /// Removes a single directory row for a path.
    fn unlink(&mut self, path: &str) -> bool {
        let path_size = self.trailer.path_size() as usize;
        let padded = {
            let mut buf = vec![0u8; path_size];
            let len = path.len().min(path_size);
            buf[..len].copy_from_slice(&path.as_bytes()[..len]);
            buf
        };

        // Find and remove the first matching directory entry.
        let pos = self.dir_entries.iter().position(|(p, _)| *p == padded);
        let Some(pos) = pos else {
            return false;
        };
        let (_, removed_id) = self.dir_entries.remove(pos);

        // If no other directory entries reference this ID, remove the entry row.
        let still_referenced = self.dir_entries.iter().any(|(_, id)| *id == removed_id);
        if !still_referenced {
            self.entry_rows.retain(|r| r.entry_id.get() != removed_id);
        }

        self.dirty = true;
        true
    }

    /// Renames an entry by updating its directory path.
    ///
    /// # Errors
    ///
    /// Returns an error if the source is not found, the destination already
    /// exists, or any resulting path would exceed path_size.
    fn rename(&mut self, from: &str, to: &str) -> Result<(), BaleError> {
        // Normalize both paths.
        let from_norm = ArchivePath::from_bytes(from.as_bytes()).normalize()?;
        let from_str = from_norm
            .as_str()
            .ok_or_else(|| BaleError::InvalidPath(from.to_string()))?;
        let to_norm = ArchivePath::from_bytes(to.as_bytes()).normalize()?;
        let to_str = to_norm
            .as_str()
            .ok_or_else(|| BaleError::InvalidPath(to.to_string()))?;

        let path_size = self.trailer.path_size() as usize;

        // Pad source to find it in dir_entries.
        let from_padded = self.pad_path(from_str)?;

        // Find the source entry.
        let source_pos = self
            .dir_entries
            .iter()
            .position(|(p, _)| *p == from_padded)
            .ok_or_else(|| BaleError::EntryNotFound(from_str.to_owned()))?;

        // Verify destination doesn't already exist.
        let to_padded = self.pad_path(to_str)?;
        if self.dir_entries.iter().any(|(p, _)| *p == to_padded) {
            return Err(BaleError::PathExists(to_str.to_owned()));
        }

        // Check if the entry is a directory.
        let entry_id = self.dir_entries[source_pos].1;
        let is_directory = self
            .find_entry_row_by_id(entry_id)
            .is_some_and(|r| r.kind() == EntryKind::Directory);

        if is_directory {
            // Collect all descendant indices and compute new paths.
            let from_prefix = format!("{from_str}/");
            let to_prefix = format!("{to_str}/");

            // First pass: validate all new paths fit within path_size.
            let mut updates: Vec<(usize, Vec<u8>)> = Vec::new();
            for (i, (path_bytes, _)) in self.dir_entries.iter().enumerate() {
                let path = ArchivePath::from_null_padded_bytes(path_bytes);
                let Some(path_str) = path.as_str() else {
                    continue;
                };
                if path_str.starts_with(&from_prefix) {
                    let suffix = &path_str[from_prefix.len()..];
                    let new_path = format!("{to_prefix}{suffix}");
                    if new_path.len() > path_size {
                        return Err(BaleError::PathTooLong {
                            path: new_path,
                            max: path_size,
                        });
                    }
                    let mut padded = vec![0u8; path_size];
                    padded[..new_path.len()].copy_from_slice(new_path.as_bytes());
                    updates.push((i, padded));
                }
            }

            // Second pass: apply all descendant renames.
            for (i, new_padded) in updates {
                self.dir_entries[i].0 = new_padded;
            }
        }

        // Update the entry's own path.
        self.dir_entries[source_pos].0 = to_padded;
        self.dirty = true;

        Ok(())
    }

    /// Replaces the content of an existing file entry.
    ///
    /// # Errors
    ///
    /// Returns an error if the entry is not found, is not a file, or writing
    /// the new data block fails.
    fn replace_content(&mut self, path: &str, data: &[u8], mode: u32) -> Result<(), BaleError> {
        // Find the entry by path.
        let (entry_row, _, entry_id) = self
            .find_entry_with_path(path)
            .ok_or_else(|| BaleError::EntryNotFound(path.to_owned()))?;

        // Must be a file.
        if entry_row.kind() != EntryKind::File {
            return Err(BaleError::NotAFile(path.to_owned()));
        }

        // Write new data block.
        let (data_offset, crc) = self.write_data_block(data)?;

        // Find and update the entry row in-place.
        let row = self
            .entry_rows
            .iter_mut()
            .find(|r| r.entry_id.get() == entry_id)
            .ok_or_else(|| BaleError::EntryNotFound(path.to_owned()))?;

        row.data_offset = U64::new(data_offset);
        row.file_size = U64::new(data.len() as u64);
        row.block_size = U64::new(data.len() as u64);
        row.crc32c = U32::new(crc.to_u32());
        row.mode = U32::new(mode);
        row.modified_time = I64::new(now_millis());

        self.dirty = true;
        Ok(())
    }

    /// Flushes all changes to disk.
    ///
    /// # Errors
    ///
    /// Returns an error if writing or syncing fails.
    fn sync(&mut self) -> Result<(), BaleError> {
        log::trace!(
            "ArchiveWriter::sync: dirty={}, write_offset={}, mmap_len={}",
            self.dirty,
            self.write_offset,
            self.mmap.len()
        );
        if !self.dirty {
            return Ok(());
        }

        // Sort entry rows by entry_id.
        self.entry_rows.sort_by_key(|r| r.entry_id.get());

        // Sort directory entries by path bytes.
        self.dir_entries.sort_by(|a, b| a.0.cmp(&b.0));

        // Reset mmap length to end of data blocks.
        self.mmap.set_len(self.write_offset)?;

        // Write entry table.
        let entry_table_offset = self.mmap.len() as u64;
        for row in &self.entry_rows {
            self.mmap.extend(row.as_bytes())?;
        }

        // Write directory table.
        let directory_table_offset = self.mmap.len() as u64;
        for (path_bytes, entry_id) in &self.dir_entries {
            self.mmap.extend(path_bytes)?;
            self.mmap.extend(&entry_id.to_le_bytes())?;
        }

        // Sync sorts and removes tombstones, so the result is compacted.
        self.trailer.set_compacted();

        // Update trailer.
        self.trailer.entry_table_offset =
            zerocopy::byteorder::little_endian::U64::new(entry_table_offset);
        self.trailer.entry_count =
            zerocopy::byteorder::little_endian::U32::new(self.entry_rows.len() as u32);
        self.trailer.directory_table_offset =
            zerocopy::byteorder::little_endian::U64::new(directory_table_offset);
        self.trailer.directory_entry_count =
            zerocopy::byteorder::little_endian::U32::new(self.dir_entries.len() as u32);

        // Set archive_size: current mmap length + trailer size.
        let archive_size = self.mmap.len() as u64 + Trailer::SIZE as u64;
        self.trailer.set_archive_size(archive_size);

        // Compute metadata CRC-32C over file header + entry table +
        // directory table + trailer bytes 0–59 (CRC field zeroed).
        self.trailer.set_metadata_crc(Crc::NONE);
        let file_header_bytes = &self.mmap.as_bytes()[..FileHeader::SIZE];
        let entry_table_bytes = {
            let offset = entry_table_offset as usize;
            let len = self.entry_rows.len() * EntryRow::SIZE;
            &self.mmap.as_bytes()[offset..offset + len]
        };
        let directory_table_bytes = {
            let offset = directory_table_offset as usize;
            &self.mmap.as_bytes()[offset..self.mmap.len()]
        };
        let metadata_crc = Trailer::compute_metadata_crc(
            file_header_bytes,
            entry_table_bytes,
            directory_table_bytes,
            &self.trailer,
        );
        self.trailer.set_metadata_crc(metadata_crc);

        // Write trailer.
        self.mmap.extend(self.trailer.as_bytes())?;

        // Flush and truncate.
        self.mmap.sync()?;
        self.dirty = false;

        Ok(())
    }

    /// Creates a directory entry.
    ///
    /// # Errors
    ///
    /// Returns an error if the path is invalid or writing fails.
    fn add_folder(&mut self, path: impl AsRef<str>, mode: u32) -> Result<(), BaleError> {
        let path = path.as_ref();
        // Ensure directory type bits are set.
        let mode = if mode & SFlag::S_IFMT.bits() == 0 {
            mode | SFlag::S_IFDIR.bits()
        } else {
            mode
        };
        self.add_entry(path, &[], mode)
    }

    /// Creates a symlink entry.
    ///
    /// The target is validated to prevent absolute paths, paths that
    /// escape the archive root, and unsafe filename components (control
    /// characters, leading dashes, components exceeding `NAME_MAX`).
    /// Validation is performed by [`resolve_target`](crate::archive::resolve_target),
    /// but the raw target string is stored as-is.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The path is invalid or writing fails
    /// - The target is absolute (`InvalidPath`)
    /// - The target escapes the archive root (`InvalidPath`)
    /// - The target contains unsafe filename components (`UnsafeFilename`)
    fn add_symlink(
        &mut self,
        path: impl AsRef<str>,
        target: impl AsRef<str>,
        mode: u32,
    ) -> Result<(), BaleError> {
        let path = path.as_ref();
        let target = target.as_ref();

        // Validate the target: rejects absolute paths, archive-root escape,
        // and unsafe filename components. The resolved path is discarded —
        // the raw target is stored as-is.
        let symlink_path = ArchivePath::try_from(path)?;
        crate::archive::resolve_target(&symlink_path, target)?;

        // Ensure symlink type bits are set.
        let mode = if mode & SFlag::S_IFMT.bits() == 0 {
            mode | SFlag::S_IFLNK.bits()
        } else {
            mode
        };
        self.add_entry(path, target.as_bytes(), mode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ArchiveReader, ArchiveWriter};
    use tempfile::TempDir;

    /// Empty archive: create + sync produces 72-byte file (header + trailer).
    #[test]
    fn empty_archive() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            assert_eq!(writer.entry_count(), 0);
            writer.sync().unwrap();
        }

        // Verify file size is file header (8) + trailer (64) = 72.
        let metadata = std::fs::metadata(&path).unwrap();
        assert_eq!(metadata.len(), 72);

        // Verify reader can open it.
        let reader = ArchiveReader::open(&path).unwrap();
        assert_eq!(reader.entry_count(), 0);
        assert!(reader.iter_entries().next().is_none());
        assert!(reader.is_sorted());
        assert!(reader.find_duplicates().is_empty());
        assert!(!reader.has_orphaned_data().unwrap());
    }

    /// Single file entry round-trips through writer and reader.
    #[test]
    fn single_file_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer
                .add_entry("hello.txt", b"Hello, world!", 0o100644)
                .unwrap();
            assert_eq!(writer.entry_count(), 1);
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&path).unwrap();
        assert_eq!(reader.entry_count(), 1);

        let entry = reader.find_entry("hello.txt").unwrap();
        assert_eq!(entry.file_size.get(), 13);
        assert_eq!(entry.mode.get(), 0o100644);
        assert_eq!(entry.kind(), EntryKind::File);

        let data = reader.read_data(entry).unwrap();
        assert_eq!(data, b"Hello, world!");

        reader.verify_crc(entry).unwrap();
    }

    /// Multiple entries are sorted and accessible via reader binary search.
    #[test]
    fn multiple_entries_sorted() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("c.txt", b"ccc", 0o100644).unwrap();
            writer.add_entry("a.txt", b"aaa", 0o100644).unwrap();
            writer.add_entry("b.txt", b"bbb", 0o100644).unwrap();
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&path).unwrap();
        assert_eq!(reader.entry_count(), 3);
        assert!(reader.is_sorted());

        // All entries findable by reader's binary search.
        assert!(reader.find_entry("a.txt").is_some());
        assert!(reader.find_entry("b.txt").is_some());
        assert!(reader.find_entry("c.txt").is_some());
        assert!(reader.find_entry("d.txt").is_none());

        // Verify data.
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
            b"bbb"
        );
        assert_eq!(
            reader
                .read_data(reader.find_entry("c.txt").unwrap())
                .unwrap(),
            b"ccc"
        );
    }

    /// add_file preserves filesystem metadata (mode, mtime).
    #[test]
    fn add_file_preserves_metadata() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("source.txt");
        std::fs::write(&src, b"file content").unwrap();

        let archive_path = dir.path().join("test.bale");
        {
            let mut writer = ArchiveWriter::create(&archive_path).unwrap();
            writer.add_file(&src, "source.txt").unwrap();
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&archive_path).unwrap();
        let entry = reader.find_entry("source.txt").unwrap();
        assert_eq!(reader.read_data(entry).unwrap(), b"file content");

        // Mode should have regular file bits set.
        assert_eq!(entry.kind(), EntryKind::File);
    }

    /// add_folder creates a directory entry with no data.
    #[test]
    fn add_folder_creates_directory() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_folder("mydir", 0o755).unwrap();
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&path).unwrap();
        let entry = reader.find_entry("mydir").unwrap();
        assert_eq!(entry.kind(), EntryKind::Directory);
        assert_eq!(entry.data_offset.get(), 0);
        assert_eq!(entry.file_size.get(), 0);

        let dir_entry = reader.folder("mydir").unwrap();
        assert_eq!(dir_entry.path().as_str(), Some("mydir"));
    }

    /// add_symlink creates a symlink entry with target as data.
    #[test]
    fn add_symlink_creates_symlink() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_symlink("link", "target/path", 0o777).unwrap();
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&path).unwrap();
        let entry = reader.find_entry("link").unwrap();
        assert_eq!(entry.kind(), EntryKind::Symlink);

        let symlink = reader.symlink("link").unwrap();
        assert_eq!(symlink.target(), Some("target/path"));
        assert_eq!(symlink.path().as_str(), Some("link"));
    }

    /// delete removes matching entries.
    #[test]
    fn delete_removes_entries() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("keep.txt", b"keep", 0o100644).unwrap();
            writer.add_entry("remove.txt", b"remove", 0o100644).unwrap();
            assert!(writer.delete("remove.txt"));
            assert!(!writer.delete("nonexistent.txt"));
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&path).unwrap();
        assert_eq!(reader.entry_count(), 1);
        assert!(reader.find_entry("keep.txt").is_some());
        assert!(reader.find_entry("remove.txt").is_none());
    }

    /// Same path added twice: both entries present (shadowing).
    #[test]
    fn shadowing_same_path_twice() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer
                .add_entry("file.txt", b"version 1", 0o100644)
                .unwrap();
            writer
                .add_entry("file.txt", b"version 2", 0o100644)
                .unwrap();
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&path).unwrap();
        // Both entries are in the tables.
        let dupes = reader.find_duplicates();
        assert_eq!(dupes.len(), 1);
        assert_eq!(dupes[0].as_str(), Some("file.txt"));

        // Directory table count includes both.
        assert_eq!(reader.trailer().directory_entry_count.get(), 2);
    }

    /// Open existing archive, add more entries, sync, verify all.
    #[test]
    fn open_existing_and_append() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        // Create initial archive.
        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("first.txt", b"first", 0o100644).unwrap();
            writer.sync().unwrap();
        }

        // Open and append.
        {
            let mut writer = ArchiveWriter::open(&path).unwrap();
            assert_eq!(writer.entry_count(), 1);
            writer.add_entry("second.txt", b"second", 0o100644).unwrap();
            writer.sync().unwrap();
        }

        // Verify all entries.
        let reader = ArchiveReader::open(&path).unwrap();
        assert_eq!(reader.entry_count(), 2);

        let first = reader.find_entry("first.txt").unwrap();
        assert_eq!(reader.read_data(first).unwrap(), b"first");

        let second = reader.find_entry("second.txt").unwrap();
        assert_eq!(reader.read_data(second).unwrap(), b"second");

        assert!(reader.is_sorted());
    }

    /// add_entry_with_mtime stores the provided timestamp.
    #[test]
    fn custom_mtime() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        let custom_time = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer
                .add_entry_with_mtime("timestamped.txt", b"data", 0o100644, Some(custom_time))
                .unwrap();
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&path).unwrap();
        let entry = reader.find_entry("timestamped.txt").unwrap();
        assert_eq!(entry.modified_time.get(), 1_700_000_000_000);
    }

    /// CRC verification passes for writer-created entries.
    #[test]
    fn crc_verification() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer
                .add_entry("data.bin", b"binary content here", 0o100644)
                .unwrap();
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&path).unwrap();
        let entry = reader.find_entry("data.bin").unwrap();
        reader.verify_crc(entry).unwrap();
    }

    /// Writer's ArchiveRead methods work before sync.
    #[test]
    fn writer_read_before_sync() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        let mut writer = ArchiveWriter::create(&path).unwrap();
        writer.add_entry("a.txt", b"alpha", 0o100644).unwrap();
        writer.add_entry("b.txt", b"beta", 0o100644).unwrap();

        // Read methods should work on in-memory tables.
        assert_eq!(writer.entry_count(), 2);
        assert!(writer.find_entry("a.txt").is_some());
        assert!(writer.find_entry("b.txt").is_some());
        assert!(writer.find_entry("c.txt").is_none());

        // Data should be readable from mmap.
        let entry = writer.find_entry("a.txt").unwrap();
        let data = writer.read_data(entry).unwrap();
        assert_eq!(data, b"alpha");

        writer.sync().unwrap();
    }

    /// Writer entry/file/folder/symlink accessors work.
    #[test]
    fn writer_typed_accessors() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        let mut writer = ArchiveWriter::create(&path).unwrap();
        writer.add_entry("a_file.txt", b"data", 0o100644).unwrap();
        writer.add_folder("a_dir", 0o755).unwrap();
        writer.add_symlink("a_link", "target", 0o777).unwrap();

        assert!(writer.entry("a_file.txt").unwrap().is_file());
        assert!(writer.entry("a_dir").unwrap().is_directory());
        assert!(writer.entry("a_link").unwrap().is_symlink());

        assert!(writer.file("a_file.txt").is_ok());
        assert!(writer.folder("a_dir").is_ok());
        assert!(writer.symlink("a_link").is_ok());

        writer.sync().unwrap();
    }

    /// find_by_id works on the writer.
    #[test]
    fn writer_find_by_id() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        let mut writer = ArchiveWriter::create(&path).unwrap();
        writer.add_entry("first.txt", b"first", 0o100644).unwrap();
        writer.add_entry("second.txt", b"second", 0o100644).unwrap();

        // Entry IDs start at 1.
        let entry1 = writer.find_by_id(1).unwrap();
        assert!(entry1.is_file());
        assert_eq!(entry1.as_file().unwrap().path().as_str(), Some("first.txt"));

        let entry2 = writer.find_by_id(2).unwrap();
        assert_eq!(
            entry2.as_file().unwrap().path().as_str(),
            Some("second.txt")
        );

        assert!(writer.find_by_id(99).is_none());

        writer.sync().unwrap();
    }

    /// Adding an entry when next_id is u32::MAX returns ArchiveFull.
    #[test]
    fn archive_full_on_id_overflow() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        let mut writer = ArchiveWriter::create(&path).unwrap();
        writer.trailer.set_next_id(u32::MAX);

        let result = writer.add_entry("overflow.txt", b"data", 0o644);
        assert!(matches!(result, Err(BaleError::ArchiveFull)));
    }

    /// Hard link creates a second path to the same entry.
    #[test]
    fn hard_link_creates_second_path() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("original.txt", b"data", 0o100644).unwrap();
            writer.hard_link("original.txt", "link.txt").unwrap();
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&path).unwrap();
        // Both paths exist and point to the same data.
        let orig = reader.file("original.txt").unwrap();
        let link = reader.file("link.txt").unwrap();
        assert_eq!(orig.data, b"data");
        assert_eq!(link.data, b"data");
        // Same entry ID.
        assert_eq!(orig.id, link.id);
        // Only one entry row.
        assert_eq!(reader.entry_count(), 1);
    }

    /// Hard link to a directory returns an error.
    #[test]
    fn hard_link_to_directory_fails() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        let mut writer = ArchiveWriter::create(&path).unwrap();
        writer.add_folder("mydir", 0o755).unwrap();

        let result = writer.hard_link("mydir", "link");
        assert!(matches!(result, Err(BaleError::NotAFile(_))));
    }

    /// Hard link to an existing path returns an error.
    #[test]
    fn hard_link_to_existing_path_fails() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        let mut writer = ArchiveWriter::create(&path).unwrap();
        writer.add_entry("a.txt", b"aaa", 0o100644).unwrap();
        writer.add_entry("b.txt", b"bbb", 0o100644).unwrap();

        let result = writer.hard_link("a.txt", "b.txt");
        assert!(matches!(result, Err(BaleError::PathExists(_))));
    }

    /// Unlink removes one path but preserves the entry when other links remain.
    #[test]
    fn unlink_removes_single_path() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("original.txt", b"data", 0o100644).unwrap();
            writer.hard_link("original.txt", "link.txt").unwrap();

            assert!(writer.unlink("original.txt"));
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&path).unwrap();
        assert!(reader.find_entry("original.txt").is_none());
        assert_eq!(reader.file("link.txt").unwrap().data, b"data");
        // Entry row still exists.
        assert_eq!(reader.entry_count(), 1);
    }

    /// Unlink of the last reference removes the entry row too.
    #[test]
    fn unlink_last_ref_removes_entry() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("only.txt", b"data", 0o100644).unwrap();

            assert!(writer.unlink("only.txt"));
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&path).unwrap();
        assert_eq!(reader.entry_count(), 0);
        assert!(reader.find_entry("only.txt").is_none());
    }

    /// Unlink of a nonexistent path returns false.
    #[test]
    fn unlink_nonexistent_returns_false() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        let mut writer = ArchiveWriter::create(&path).unwrap();
        assert!(!writer.unlink("nope.txt"));
    }

    /// Rename updates the path for an entry.
    #[test]
    fn rename_updates_path() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("old.txt", b"data", 0o100644).unwrap();
            writer.rename("old.txt", "new.txt").unwrap();
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&path).unwrap();
        assert!(reader.find_entry("old.txt").is_none());
        assert_eq!(reader.file("new.txt").unwrap().data, b"data");
    }

    /// Rename of a directory updates all descendant paths.
    #[test]
    fn rename_directory_updates_descendants() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_folder("src", 0o755).unwrap();
            writer.add_entry("src/a.txt", b"aaa", 0o100644).unwrap();
            writer.add_entry("src/b.txt", b"bbb", 0o100644).unwrap();
            writer.rename("src", "dst").unwrap();
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&path).unwrap();
        assert!(reader.find_entry("src").is_none());
        assert!(reader.find_entry("src/a.txt").is_none());
        assert!(reader.find_entry("src/b.txt").is_none());
        assert!(reader.folder("dst").is_ok());
        assert_eq!(reader.file("dst/a.txt").unwrap().data, b"aaa");
        assert_eq!(reader.file("dst/b.txt").unwrap().data, b"bbb");
    }

    /// Rename to an existing path returns an error.
    #[test]
    fn rename_to_existing_path_fails() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        let mut writer = ArchiveWriter::create(&path).unwrap();
        writer.add_entry("a.txt", b"aaa", 0o100644).unwrap();
        writer.add_entry("b.txt", b"bbb", 0o100644).unwrap();

        let result = writer.rename("a.txt", "b.txt");
        assert!(matches!(result, Err(BaleError::PathExists(_))));
    }

    /// Rename that would exceed path_size fails atomically.
    #[test]
    fn rename_exceeding_path_size_fails() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        // Create archive with small path_size.
        let mut writer = ArchiveWriter::create_with_options(&path, 4096, 16).unwrap();
        writer.add_folder("a", 0o755).unwrap();
        writer.add_entry("a/file.txt", b"data", 0o100644).unwrap();

        // Renaming "a" to a long name would make "a/file.txt" exceed 16 bytes.
        let result = writer.rename("a", "very_long_name");
        assert!(matches!(result, Err(BaleError::PathTooLong { .. })));

        // Original paths are preserved (atomic rejection).
        assert!(writer.find_entry("a").is_some());
        assert!(writer.find_entry("a/file.txt").is_some());
    }

    /// replace_content updates the data for an existing file.
    #[test]
    fn replace_content_updates_data() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            writer.add_entry("file.txt", b"old data", 0o100644).unwrap();
            writer
                .replace_content("file.txt", b"new data!", 0o100755)
                .unwrap();
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&path).unwrap();
        let file = reader.file("file.txt").unwrap();
        assert_eq!(file.data, b"new data!");
        assert_eq!(file.entry.mode.get(), 0o100755);
        reader.verify_crc(file.entry).unwrap();
    }

    /// add_symlink rejects absolute targets starting with `/`.
    #[test]
    fn add_symlink_rejects_absolute_target() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        let mut writer = ArchiveWriter::create(&path).unwrap();
        let result = writer.add_symlink("link", "/usr/share/data.txt", 0o777);
        assert!(matches!(result, Err(BaleError::InvalidPath(_))));
    }

    /// add_symlink rejects targets that escape the archive root via `..`.
    #[test]
    fn add_symlink_rejects_escaping_target() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        let mut writer = ArchiveWriter::create(&path).unwrap();
        let result = writer.add_symlink("link", "../../outside.txt", 0o777);
        assert!(matches!(result, Err(BaleError::InvalidPath(_))));
    }

    /// add_symlink allows relative targets that stay within the archive.
    #[test]
    fn add_symlink_allows_relative_target() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        {
            let mut writer = ArchiveWriter::create(&path).unwrap();
            // dir/link -> ../sibling resolves to "sibling", which is valid.
            writer.add_symlink("dir/link", "../sibling", 0o777).unwrap();
            writer.sync().unwrap();
        }

        let reader = ArchiveReader::open(&path).unwrap();
        let symlink = reader.symlink("dir/link").unwrap();
        assert_eq!(symlink.target(), Some("../sibling"));
    }

    /// add_symlink rejects targets with components exceeding NAME_MAX (255).
    ///
    /// Symlink target components are validated with safename rules, which
    /// enforce the standard NAME_MAX limit per component.
    #[test]
    fn add_symlink_rejects_long_component_in_target() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        // Create a target with a 257-byte component (exceeds safename's 255 limit).
        let long_component = "a".repeat(257);
        let target = format!("{long_component}/file.txt");

        let mut writer = ArchiveWriter::create(&path).unwrap();
        let result = writer.add_symlink("link", &target, 0o777);
        assert!(matches!(result, Err(BaleError::UnsafeFilename(_))));
    }

    /// add_symlink rejects targets with control characters in components.
    #[test]
    fn add_symlink_rejects_control_chars_in_target() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        let mut writer = ArchiveWriter::create(&path).unwrap();
        let result = writer.add_symlink("link", "foo\x01bar", 0o777);
        assert!(matches!(result, Err(BaleError::UnsafeFilename(_))));
    }

    /// replace_content on a directory returns an error.
    #[test]
    fn replace_content_on_directory_fails() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bale");

        let mut writer = ArchiveWriter::create(&path).unwrap();
        writer.add_folder("mydir", 0o755).unwrap();

        let result = writer.replace_content("mydir", b"data", 0o100644);
        assert!(matches!(result, Err(BaleError::NotAFile(_))));
    }
}
