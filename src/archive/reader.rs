//! Read-only archive implementation.
//!
//! Provides zero-copy reading of v1.0.0 format archives. All data is accessed
//! directly from the memory-mapped file via offsets stored in the trailer.

use super::{Archive, ArchiveRead, DirEntry, Entry, FileEntry, SymlinkEntry};
use crate::format::{Crc, DirectoryRow, EntryRow, FileHeader, Trailer};
use crate::{ArchivePath, BaleError, EntryKind, MappedArchive};
use std::collections::HashSet;
use std::path::Path;
use zerocopy::FromBytes;

impl Archive<MappedArchive> {
    /// Opens an archive for reading.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file cannot be opened or memory-mapped
    /// - The archive is too small to contain a valid trailer
    /// - The trailer magic or version is invalid
    /// - The metadata CRC-32C does not match
    pub fn open(path: impl AsRef<Path>) -> Result<Self, BaleError> {
        let mmap = MappedArchive::open(path)?;
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

        // Validate metadata CRC-32C.
        Self::check_metadata_crc(bytes, &trailer)?;

        Ok(Self {
            mmap,
            trailer,
            write_offset: 0,
            dirty: false,
            entry_rows: Vec::new(),
            dir_entries: Vec::new(),
        })
    }

    /// Verifies the metadata CRC-32C against the archive contents.
    ///
    /// Computes the CRC over file header, entry table, directory table,
    /// and trailer bytes 0–59, then compares against the stored value.
    ///
    /// # Errors
    ///
    /// Returns `BaleError::Corrupted` if the CRC does not match.
    fn check_metadata_crc(bytes: &[u8], trailer: &Trailer) -> Result<(), BaleError> {
        let stored = trailer.metadata_crc();
        let file_header = &bytes[..FileHeader::SIZE];
        let entry_table = {
            let count = trailer.entry_count.get() as usize;
            if count == 0 {
                &[] as &[u8]
            } else {
                let offset = trailer.entry_table_offset.get() as usize;
                let len = count * EntryRow::SIZE;
                &bytes[offset..offset + len]
            }
        };
        let directory_table = {
            let count = trailer.directory_entry_count.get() as usize;
            if count == 0 {
                &[] as &[u8]
            } else {
                let offset = trailer.directory_table_offset.get() as usize;
                let stride = DirectoryRow::stride(trailer.path_size());
                let len = count * stride;
                &bytes[offset..offset + len]
            }
        };

        // Compute with CRC field zeroed (use a copy of the trailer).
        let computed = {
            let mut trailer_for_crc = *trailer;
            trailer_for_crc.set_metadata_crc(Crc::NONE);
            Trailer::compute_metadata_crc(
                file_header,
                entry_table,
                directory_table,
                &trailer_for_crc,
            )
        };

        if stored != computed {
            let stored_val = stored.to_u32();
            let computed_val = computed.to_u32();
            return Err(BaleError::Corrupted(format!(
                "metadata CRC-32C mismatch: stored {stored_val:#010x}, computed {computed_val:#010x}"
            )));
        }
        Ok(())
    }

    /// Returns a zero-copy slice of the entry table from the mmap.
    ///
    /// Returns an empty slice when the archive has no entries.
    ///
    /// # Errors
    ///
    /// Returns an error if the entry table bytes cannot be interpreted as
    /// a valid `[EntryRow]` slice (e.g., corrupted archive data).
    fn entry_table(&self) -> Result<&[EntryRow], BaleError> {
        let count = self.trailer.entry_count.get() as usize;
        if count == 0 {
            return Ok(&[]);
        }
        let offset = self.trailer.entry_table_offset.get() as usize;
        let len = count * EntryRow::SIZE;
        let bytes = &self.mmap.as_bytes()[offset..offset + len];
        <[EntryRow]>::ref_from_bytes(bytes)
            .map_err(|e| BaleError::Corrupted(format!("invalid entry table: {e}")))
    }

    /// Returns the raw bytes of the directory table from the mmap.
    fn directory_table_bytes(&self) -> &[u8] {
        let count = self.trailer.directory_entry_count.get() as usize;
        if count == 0 {
            return &[];
        }
        let offset = self.trailer.directory_table_offset.get() as usize;
        let stride = DirectoryRow::stride(self.trailer.path_size());
        let len = count * stride;
        &self.mmap.as_bytes()[offset..offset + len]
    }

    /// Returns the directory row at the given index.
    ///
    /// Returns `None` if the index is out of bounds.
    fn directory_row(&self, index: usize) -> Option<DirectoryRow<'_>> {
        let count = self.trailer.directory_entry_count.get() as usize;
        if index >= count {
            return None;
        }
        let stride = DirectoryRow::stride(self.trailer.path_size());
        let table = self.directory_table_bytes();
        let start = index * stride;
        DirectoryRow::from_bytes(&table[start..start + stride], self.trailer.path_size()).ok()
    }

    /// Finds an entry row by ID using binary search on the entry table.
    ///
    /// Returns `None` if the ID is not found or the entry table is corrupted.
    fn find_entry_row_by_id(&self, id: u32) -> Option<&EntryRow> {
        let table = self.entry_table().ok()?;
        table
            .binary_search_by_key(&id, |row| row.entry_id.get())
            .ok()
            .map(|idx| &table[idx])
    }

    /// Compares a null-padded stored path against a search key without allocating.
    ///
    /// The stored path is `path_size` bytes (null-padded). The search key is
    /// the raw path string bytes.
    fn cmp_path_bytes(stored: &[u8], search: &[u8]) -> std::cmp::Ordering {
        for (i, &stored_byte) in stored.iter().enumerate() {
            let search_byte = if i < search.len() { search[i] } else { 0 };
            match stored_byte.cmp(&search_byte) {
                std::cmp::Ordering::Equal => continue,
                other => return other,
            }
        }
        // If the search key is longer than path_size, there cannot be a match,
        // but the stored path sorts before the search key.
        if search.len() > stored.len() {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Equal
        }
    }

    /// Finds a directory row by path.
    ///
    /// Uses binary search when the compacted flag is set (directory table is
    /// sorted with no tombstones). Falls back to linear scan when clear,
    /// skipping tombstoned rows (entry_id == 0).
    fn find_directory_row_by_path(&self, path: &str) -> Option<(DirectoryRow<'_>, u32)> {
        let path_size = self.trailer.path_size() as usize;
        let stride = DirectoryRow::stride(self.trailer.path_size());
        let table = self.directory_table_bytes();
        let search = path.as_bytes();

        if self.trailer.is_compacted() {
            // Binary search: directory table is sorted with no tombstones.
            let rows: Vec<&[u8]> = table.chunks_exact(stride).collect();
            let result =
                rows.binary_search_by(|row| Self::cmp_path_bytes(&row[..path_size], search));
            match result {
                Ok(idx) => {
                    let row = self.directory_row(idx)?;
                    let entry_id = row.entry_id();
                    Some((row, entry_id))
                }
                Err(_) => None,
            }
        } else {
            // Linear scan: table may be unsorted or contain tombstones.
            for chunk in table.chunks_exact(stride) {
                if Self::cmp_path_bytes(&chunk[..path_size], search) == std::cmp::Ordering::Equal {
                    let row = DirectoryRow::from_bytes(&chunk[..stride], self.trailer.path_size())
                        .ok()?;
                    let entry_id = row.entry_id();
                    // Skip tombstoned rows (entry_id == 0).
                    if entry_id == 0 {
                        continue;
                    }
                    return Some((row, entry_id));
                }
            }
            None
        }
    }

    /// Constructs an `Entry` from an entry row, path bytes, and ID.
    ///
    /// All references must borrow from the same archive (i.e., `self`).
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
}

impl ArchiveRead for Archive<MappedArchive> {
    /// Returns the number of entries in the archive.
    fn entry_count(&self) -> usize {
        self.trailer.entry_count.get() as usize
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
        let row = self.directory_row(index)?;
        Some(ArchivePath::from_null_padded_bytes(row.path_bytes_raw()))
    }

    /// Iterates over all entry rows with their null-padded path bytes.
    fn iter_entries(&self) -> impl Iterator<Item = (&EntryRow, &[u8])> {
        let count = self.trailer.directory_entry_count.get() as usize;
        let stride = DirectoryRow::stride(self.trailer.path_size());
        let path_size = self.trailer.path_size();
        let table = self.directory_table_bytes();

        (0..count).filter_map(move |i| {
            let start = i * stride;
            let row = DirectoryRow::from_bytes(&table[start..start + stride], path_size).ok()?;
            let entry_id = row.entry_id();
            // Skip tombstoned rows (entry_id == 0).
            if entry_id == 0 {
                return None;
            }
            let entry_row = self.find_entry_row_by_id(entry_id)?;
            Some((entry_row, row.path_bytes_raw()))
        })
    }

    /// Finds an entry row by path using binary search on the directory table.
    fn find_entry(&self, path: &str) -> Option<&EntryRow> {
        let (_, entry_id) = self.find_directory_row_by_path(path)?;
        self.find_entry_row_by_id(entry_id)
    }

    /// Finds an entry by path and returns entry row, trimmed path bytes, and ID.
    fn find_entry_with_path(&self, path: &str) -> Option<(&EntryRow, &[u8], u32)> {
        let (dir_row, entry_id) = self.find_directory_row_by_path(path)?;
        let entry_row = self.find_entry_row_by_id(entry_id)?;
        Some((entry_row, dir_row.path_bytes(), entry_id))
    }

    /// Reads the data bytes for the given entry row.
    ///
    /// Returns an empty slice for entries with no data block (directories).
    ///
    /// # Errors
    ///
    /// Returns an error if the data offset or size is out of bounds.
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
    /// Returns an error if the data cannot be read or the CRC does not match.
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
    /// # Errors
    ///
    /// Returns an error if the computed CRC does not match the stored CRC.
    fn verify_metadata_crc(&self) -> Result<(), BaleError> {
        Self::check_metadata_crc(self.mmap.as_bytes(), &self.trailer)
    }

    /// Checks if the directory table is sorted by raw path bytes.
    fn is_sorted(&self) -> bool {
        let count = self.trailer.directory_entry_count.get() as usize;
        if count <= 1 {
            return true;
        }
        let stride = DirectoryRow::stride(self.trailer.path_size());
        let path_size = self.trailer.path_size() as usize;
        let table = self.directory_table_bytes();

        (1..count).all(|i| {
            let prev_start = (i - 1) * stride;
            let curr_start = i * stride;
            let prev_path = &table[prev_start..prev_start + path_size];
            let curr_path = &table[curr_start..curr_start + path_size];
            prev_path <= curr_path
        })
    }

    /// Returns duplicate paths in the directory table.
    fn find_duplicates(&self) -> Vec<ArchivePath<'static>> {
        let count = self.trailer.directory_entry_count.get() as usize;
        if count <= 1 {
            return Vec::new();
        }
        let stride = DirectoryRow::stride(self.trailer.path_size());
        let path_size = self.trailer.path_size() as usize;
        let table = self.directory_table_bytes();
        let mut duplicates = Vec::new();
        let mut seen = HashSet::new();

        for i in 1..count {
            let prev_start = (i - 1) * stride;
            let curr_start = i * stride;
            let prev_path = &table[prev_start..prev_start + path_size];
            let curr_path = &table[curr_start..curr_start + path_size];

            if prev_path == curr_path && seen.insert(curr_path.to_vec()) {
                let path = ArchivePath::from_null_padded_bytes(curr_path);
                duplicates.push(path.into_owned());
            }
        }
        duplicates
    }

    /// Checks for orphaned data blocks not referenced by any entry.
    ///
    /// # Errors
    ///
    /// Returns an error if the entry table or alignment is corrupted.
    fn has_orphaned_data(&self) -> Result<bool, BaleError> {
        let entry_table = self.entry_table()?;
        let referenced: HashSet<u64> = entry_table
            .iter()
            .map(|row| row.data_offset.get())
            .filter(|&offset| offset != 0)
            .collect();

        let alignment = self.alignment()? as u64;
        let data_region_end = self.trailer.entry_table_offset.get();

        // Walk aligned offsets from the first alignment boundary after the file
        // header up to the entry table.
        let mut offset = {
            let start = FileHeader::SIZE as u64;
            if alignment == 0 {
                return Ok(false);
            }
            // Round up to next alignment boundary.
            start.div_ceil(alignment) * alignment
        };

        while offset < data_region_end {
            // Each alignment boundary in the data region that is not referenced
            // by any entry is an orphaned data block.
            if !referenced.contains(&offset) {
                return Ok(true);
            }
            offset += alignment;
        }
        Ok(false)
    }

    /// Returns a file entry by path.
    ///
    /// # Errors
    ///
    /// Returns an error if the path is not found or is not a file.
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
    /// Returns an error if the path is not found or is not a directory.
    fn directory(&self, path: impl AsRef<str>) -> Result<DirEntry<'_>, BaleError> {
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
    /// Returns an error if the path is not found or is not a symlink.
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
    /// Returns an error if the path is not found.
    fn entry(&self, path: impl AsRef<str>) -> Result<Entry<'_>, BaleError> {
        let path = path.as_ref();
        let (entry, path_bytes, id) = self
            .find_entry_with_path(path)
            .ok_or_else(|| BaleError::EntryNotFound(path.to_owned()))?;
        self.make_entry(entry, path_bytes, id)
    }

    /// Finds an entry by its stable ID.
    ///
    /// Binary searches the entry table, then linear scans the directory table
    /// for the matching entry ID to recover the path.
    fn find_by_id(&self, id: u32) -> Option<Entry<'_>> {
        let entry_row = self.find_entry_row_by_id(id)?;

        // Linear scan directory table for this entry ID.
        let count = self.trailer.directory_entry_count.get() as usize;
        let stride = DirectoryRow::stride(self.trailer.path_size());
        let path_size = self.trailer.path_size();
        let table = self.directory_table_bytes();

        for i in 0..count {
            let start = i * stride;
            if let Ok(row) = DirectoryRow::from_bytes(&table[start..start + stride], path_size)
                && row.entry_id() == id
            {
                return self.make_entry(entry_row, row.path_bytes(), id).ok();
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{Crc, FileHeader};
    use zerocopy::IntoBytes;

    /// Default path size used in tests.
    const TEST_PATH_SIZE: u16 = 256;

    /// Default alignment used in tests (4096 bytes).
    const TEST_ALIGNMENT: u32 = 4096;

    /// Test entry representing a file, directory, or symlink for archive construction.
    struct TestEntry {
        /// The entry path (must fit within TEST_PATH_SIZE).
        path: &'static str,
        /// The file data bytes.
        data: &'static [u8],
        /// Unix mode (determines entry kind).
        mode: u32,
        /// Stable entry ID.
        id: u32,
    }

    /// Builds a v2 format archive in memory from a list of test entries.
    ///
    /// Returns the raw archive bytes suitable for memory-mapped reading.
    /// Entries are placed in the directory table in the order given (caller
    /// controls sort order). The entry table is always sorted by ID.
    fn build_test_archive(entries: &[TestEntry]) -> Vec<u8> {
        build_test_archive_with_options(entries, TEST_PATH_SIZE, TEST_ALIGNMENT)
    }

    /// Builds a v2 format archive with configurable path size and alignment.
    fn build_test_archive_with_options(
        entries: &[TestEntry],
        path_size: u16,
        alignment: u32,
    ) -> Vec<u8> {
        let mut buf = Vec::new();

        // 1. File header (8 bytes).
        let file_header = FileHeader::new();
        buf.extend_from_slice(file_header.as_bytes());

        // 2. Data blocks (aligned, raw bytes with no per-block header).
        struct DataInfo {
            /// Offset in the archive where the data block starts.
            offset: u64,
            /// CRC-32 of the data bytes.
            crc: Crc,
        }
        let mut data_infos: Vec<DataInfo> = Vec::new();

        for entry in entries {
            if entry.data.is_empty() {
                data_infos.push(DataInfo {
                    offset: 0,
                    crc: Crc::NONE,
                });
                continue;
            }
            // Pad to alignment.
            let alignment = alignment as usize;
            let current = buf.len();
            let padding = (alignment - (current % alignment)) % alignment;
            buf.extend(std::iter::repeat_n(0u8, padding));

            let data_offset = buf.len() as u64;
            let crc = Crc::compute(entry.data);

            // Write raw data (no header).
            buf.extend_from_slice(entry.data);

            data_infos.push(DataInfo {
                offset: data_offset,
                crc,
            });
        }

        // 3. Entry table (sorted by ID).
        let mut entry_rows: Vec<(u32, EntryRow)> = entries
            .iter()
            .zip(data_infos.iter())
            .map(|(e, di)| {
                let block_size = if e.data.is_empty() {
                    0
                } else {
                    e.data.len() as u64
                };
                let row = EntryRow::new_file(
                    e.id,
                    di.crc,
                    di.offset,
                    e.data.len() as u64,
                    block_size,
                    1_700_000_000_000,
                    1_700_000_001_000,
                    e.mode,
                );
                (e.id, row)
            })
            .collect();
        entry_rows.sort_by_key(|(id, _)| *id);

        let entry_table_offset = buf.len() as u64;
        for (_, row) in &entry_rows {
            buf.extend_from_slice(row.as_bytes());
        }
        let entry_count = entry_rows.len() as u32;

        // 4. Directory table (in given order — caller controls sorting).
        let directory_table_offset = buf.len() as u64;
        let stride = DirectoryRow::stride(path_size);
        for entry in entries {
            let mut row_bytes = vec![0u8; stride];
            let path_bytes = entry.path.as_bytes();
            let copy_len = path_bytes.len().min(path_size as usize);
            row_bytes[..copy_len].copy_from_slice(&path_bytes[..copy_len]);
            let id_offset = path_size as usize;
            row_bytes[id_offset..id_offset + 4].copy_from_slice(&entry.id.to_le_bytes());
            buf.extend_from_slice(&row_bytes);
        }
        let directory_entry_count = entries.len() as u32;

        // 5. Trailer (64 bytes).
        let alignment_power = alignment.trailing_zeros() as u8;
        let next_id = entries.iter().map(|e| e.id).max().unwrap_or(0) + 1;
        let mut trailer = Trailer::new();
        trailer.entry_table_offset =
            zerocopy::byteorder::little_endian::U64::new(entry_table_offset);
        trailer.entry_count = zerocopy::byteorder::little_endian::U32::new(entry_count);
        trailer.directory_table_offset =
            zerocopy::byteorder::little_endian::U64::new(directory_table_offset);
        trailer.directory_entry_count =
            zerocopy::byteorder::little_endian::U32::new(directory_entry_count);
        trailer.next_id = zerocopy::byteorder::little_endian::U32::new(next_id);
        trailer.alignment_power = alignment_power;
        trailer.path_size = zerocopy::byteorder::little_endian::U16::new(path_size);

        // Set compacted flag only if the directory table is sorted.
        let is_sorted = entries.windows(2).all(|w| w[0].path <= w[1].path);
        if is_sorted {
            trailer.set_compacted();
        } else {
            trailer.clear_compacted();
        }

        // Set archive_size to final file size.
        let archive_size = (buf.len() + Trailer::SIZE) as u64;
        trailer.set_archive_size(archive_size);

        // Compute metadata CRC-32C.
        let file_header_bytes = &buf[..FileHeader::SIZE];
        let entry_table_bytes = {
            let offset = entry_table_offset as usize;
            let len = entry_count as usize * EntryRow::SIZE;
            &buf[offset..offset + len]
        };
        let directory_table_bytes = {
            let offset = directory_table_offset as usize;
            let len = directory_entry_count as usize * stride;
            &buf[offset..offset + len]
        };
        let metadata_crc = Trailer::compute_metadata_crc(
            file_header_bytes,
            entry_table_bytes,
            directory_table_bytes,
            &trailer,
        );
        trailer.set_metadata_crc(metadata_crc);

        buf.extend_from_slice(trailer.as_bytes());

        buf
    }

    /// Creates an `Archive<MappedArchive>` from raw bytes by writing to a temp file.
    fn archive_from_bytes(bytes: &[u8]) -> Archive<MappedArchive> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.bale");
        std::fs::write(&path, bytes).unwrap();
        // Keep tempdir alive by leaking it (tests are short-lived).
        let path = path.to_owned();
        std::mem::forget(dir);
        <Archive<MappedArchive>>::open(path).unwrap()
    }

    /// Empty archive has zero entries and empty iterators.
    #[test]
    fn empty_archive() {
        let bytes = build_test_archive(&[]);
        let archive = archive_from_bytes(&bytes);

        assert_eq!(archive.entry_count(), 0);
        assert_eq!(archive.path_size(), TEST_PATH_SIZE as usize);
        assert_eq!(archive.alignment().unwrap(), TEST_ALIGNMENT);
        assert!(archive.iter_entries().next().is_none());
        assert!(archive.find_entry("hello.txt").is_none());
        assert!(archive.get_path(0).is_none());
        assert!(archive.is_sorted());
        assert!(archive.find_duplicates().is_empty());
        assert!(!archive.has_orphaned_data().unwrap());
    }

    /// Single file entry can be found and read.
    #[test]
    fn single_file_read() {
        let bytes = build_test_archive(&[TestEntry {
            path: "hello.txt",
            data: b"Hello, world!",
            mode: 0o100644,
            id: 1,
        }]);
        let archive = archive_from_bytes(&bytes);

        assert_eq!(archive.entry_count(), 1);

        let entry = archive.find_entry("hello.txt").unwrap();
        assert_eq!(entry.entry_id.get(), 1);
        assert_eq!(entry.file_size.get(), 13);
        assert_eq!(entry.kind(), EntryKind::File);

        let data = archive.read_data(entry).unwrap();
        assert_eq!(data, b"Hello, world!");

        archive.verify_crc(entry).unwrap();
    }

    /// get_path returns the path at an index.
    #[test]
    fn get_path_returns_path() {
        let bytes = build_test_archive(&[TestEntry {
            path: "foo.txt",
            data: b"data",
            mode: 0o100644,
            id: 1,
        }]);
        let archive = archive_from_bytes(&bytes);

        let path = archive.get_path(0).unwrap();
        assert_eq!(path.as_str(), Some("foo.txt"));
        assert!(archive.get_path(1).is_none());
    }

    /// iter_entries yields all entries with correct paths.
    #[test]
    fn iter_entries_yields_all() {
        let bytes = build_test_archive(&[
            TestEntry {
                path: "a.txt",
                data: b"aaa",
                mode: 0o100644,
                id: 1,
            },
            TestEntry {
                path: "b.txt",
                data: b"bbb",
                mode: 0o100644,
                id: 2,
            },
        ]);
        let archive = archive_from_bytes(&bytes);

        let entries: Vec<_> = archive.iter_entries().collect();
        assert_eq!(entries.len(), 2);

        let path0 = ArchivePath::from_null_padded_bytes(entries[0].1);
        assert_eq!(path0.as_str(), Some("a.txt"));

        let path1 = ArchivePath::from_null_padded_bytes(entries[1].1);
        assert_eq!(path1.as_str(), Some("b.txt"));
    }

    /// find_entry_with_path returns trimmed path bytes and ID.
    #[test]
    fn find_entry_with_path_returns_trimmed() {
        let bytes = build_test_archive(&[TestEntry {
            path: "test.dat",
            data: b"data",
            mode: 0o100644,
            id: 5,
        }]);
        let archive = archive_from_bytes(&bytes);

        let (entry, path_bytes, id) = archive.find_entry_with_path("test.dat").unwrap();
        assert_eq!(id, 5);
        assert_eq!(path_bytes, b"test.dat");
        assert_eq!(entry.entry_id.get(), 5);
    }

    /// Nonexistent path returns None.
    #[test]
    fn find_entry_missing() {
        let bytes = build_test_archive(&[TestEntry {
            path: "exists.txt",
            data: b"data",
            mode: 0o100644,
            id: 1,
        }]);
        let archive = archive_from_bytes(&bytes);

        assert!(archive.find_entry("missing.txt").is_none());
        assert!(archive.find_entry_with_path("missing.txt").is_none());
    }

    /// Multiple entries with binary search on directory and entry tables.
    #[test]
    fn multi_entry_binary_search() {
        let bytes = build_test_archive(&[
            TestEntry {
                path: "alpha",
                data: b"a",
                mode: 0o100644,
                id: 1,
            },
            TestEntry {
                path: "beta",
                data: b"b",
                mode: 0o100644,
                id: 2,
            },
            TestEntry {
                path: "gamma",
                data: b"g",
                mode: 0o100644,
                id: 3,
            },
        ]);
        let archive = archive_from_bytes(&bytes);

        assert_eq!(archive.entry_count(), 3);
        assert!(archive.find_entry("alpha").is_some());
        assert!(archive.find_entry("beta").is_some());
        assert!(archive.find_entry("gamma").is_some());
        assert!(archive.find_entry("delta").is_none());
    }

    /// Sorted directory table is detected as sorted.
    #[test]
    fn is_sorted_when_sorted() {
        let bytes = build_test_archive(&[
            TestEntry {
                path: "aaa",
                data: b"a",
                mode: 0o100644,
                id: 1,
            },
            TestEntry {
                path: "bbb",
                data: b"b",
                mode: 0o100644,
                id: 2,
            },
            TestEntry {
                path: "ccc",
                data: b"c",
                mode: 0o100644,
                id: 3,
            },
        ]);
        let archive = archive_from_bytes(&bytes);
        assert!(archive.is_sorted());
    }

    /// Unsorted directory table is detected and uses linear scan.
    #[test]
    fn is_sorted_when_unsorted() {
        let bytes = build_test_archive(&[
            TestEntry {
                path: "ccc",
                data: b"c",
                mode: 0o100644,
                id: 3,
            },
            TestEntry {
                path: "aaa",
                data: b"a",
                mode: 0o100644,
                id: 1,
            },
            TestEntry {
                path: "bbb",
                data: b"b",
                mode: 0o100644,
                id: 2,
            },
        ]);
        let archive = archive_from_bytes(&bytes);
        assert!(!archive.is_sorted());
        assert!(!archive.trailer().is_compacted());

        // Linear scan still finds all entries.
        assert!(archive.find_entry("aaa").is_some());
        assert!(archive.find_entry("bbb").is_some());
        assert!(archive.find_entry("ccc").is_some());
        assert!(archive.find_entry("ddd").is_none());

        // Data is correct.
        let entry = archive.find_entry("bbb").unwrap();
        assert_eq!(archive.read_data(entry).unwrap(), b"b");
    }

    /// Duplicate paths are detected.
    #[test]
    fn find_duplicates_detects_dupes() {
        let bytes = build_test_archive(&[
            TestEntry {
                path: "dup.txt",
                data: b"first",
                mode: 0o100644,
                id: 1,
            },
            TestEntry {
                path: "dup.txt",
                data: b"second",
                mode: 0o100644,
                id: 2,
            },
            TestEntry {
                path: "unique.txt",
                data: b"only",
                mode: 0o100644,
                id: 3,
            },
        ]);
        let archive = archive_from_bytes(&bytes);

        let dupes = archive.find_duplicates();
        assert_eq!(dupes.len(), 1);
        assert_eq!(dupes[0].as_str(), Some("dup.txt"));
    }

    /// No duplicates when all paths are unique.
    #[test]
    fn find_duplicates_empty_when_unique() {
        let bytes = build_test_archive(&[
            TestEntry {
                path: "a.txt",
                data: b"a",
                mode: 0o100644,
                id: 1,
            },
            TestEntry {
                path: "b.txt",
                data: b"b",
                mode: 0o100644,
                id: 2,
            },
        ]);
        let archive = archive_from_bytes(&bytes);
        assert!(archive.find_duplicates().is_empty());
    }

    /// CRC mismatch is detected when data is corrupted.
    #[test]
    fn verify_crc_mismatch() {
        let mut bytes = build_test_archive(&[TestEntry {
            path: "corrupt.txt",
            data: b"Hello, world!",
            mode: 0o100644,
            id: 1,
        }]);

        // Corrupt a data byte. Data starts directly at the first alignment
        // boundary (4096) with no per-block header.
        let data_byte_offset = TEST_ALIGNMENT as usize;
        bytes[data_byte_offset] ^= 0xFF;

        let archive = archive_from_bytes(&bytes);
        let entry = archive.find_entry("corrupt.txt").unwrap();
        let result = archive.verify_crc(entry);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("CRC-32C mismatch"), "error was: {err}");
    }

    /// verify_crc succeeds for a directory (no data block).
    #[test]
    fn verify_crc_directory_ok() {
        let bytes = build_test_archive(&[TestEntry {
            path: "mydir",
            data: b"",
            mode: 0o040755,
            id: 1,
        }]);
        let archive = archive_from_bytes(&bytes);
        let entry = archive.find_entry("mydir").unwrap();
        archive.verify_crc(entry).unwrap();
    }

    /// file() returns a FileEntry for file entries.
    #[test]
    fn file_accessor() {
        let bytes = build_test_archive(&[TestEntry {
            path: "readme.txt",
            data: b"Read me",
            mode: 0o100644,
            id: 1,
        }]);
        let archive = archive_from_bytes(&bytes);

        let file = archive.file("readme.txt").unwrap();
        assert_eq!(file.data(), b"Read me");
        assert_eq!(file.path().as_str(), Some("readme.txt"));
        assert_eq!(file.id(), 1);
    }

    /// file() on a directory returns NotAFile.
    #[test]
    fn file_on_directory_returns_not_a_file() {
        let bytes = build_test_archive(&[TestEntry {
            path: "mydir",
            data: b"",
            mode: 0o040755,
            id: 1,
        }]);
        let archive = archive_from_bytes(&bytes);

        let result = archive.file("mydir");
        assert!(matches!(result, Err(BaleError::NotAFile(_))));
    }

    /// directory() returns a DirEntry for directories.
    #[test]
    fn directory_accessor() {
        let bytes = build_test_archive(&[TestEntry {
            path: "src",
            data: b"",
            mode: 0o040755,
            id: 1,
        }]);
        let archive = archive_from_bytes(&bytes);

        let dir = archive.directory("src").unwrap();
        assert_eq!(dir.path().as_str(), Some("src"));
        assert_eq!(dir.id(), 1);
    }

    /// directory() on a file returns NotADirectory.
    #[test]
    fn directory_on_file_returns_not_a_directory() {
        let bytes = build_test_archive(&[TestEntry {
            path: "file.txt",
            data: b"data",
            mode: 0o100644,
            id: 1,
        }]);
        let archive = archive_from_bytes(&bytes);

        let result = archive.directory("file.txt");
        assert!(matches!(result, Err(BaleError::NotADirectory(_))));
    }

    /// symlink() returns a SymlinkEntry.
    #[test]
    fn symlink_accessor() {
        let bytes = build_test_archive(&[TestEntry {
            path: "link",
            data: b"target/path",
            mode: 0o120777,
            id: 1,
        }]);
        let archive = archive_from_bytes(&bytes);

        let symlink = archive.symlink("link").unwrap();
        assert_eq!(symlink.target(), Some("target/path"));
        assert_eq!(symlink.path().as_str(), Some("link"));
    }

    /// symlink() on a file returns NotASymlink.
    #[test]
    fn symlink_on_file_returns_not_a_symlink() {
        let bytes = build_test_archive(&[TestEntry {
            path: "file.txt",
            data: b"data",
            mode: 0o100644,
            id: 1,
        }]);
        let archive = archive_from_bytes(&bytes);

        let result = archive.symlink("file.txt");
        assert!(matches!(result, Err(BaleError::NotASymlink(_))));
    }

    /// entry() returns the correct variant for each kind.
    #[test]
    fn entry_accessor_dispatches_correctly() {
        let bytes = build_test_archive(&[
            TestEntry {
                path: "a_dir",
                data: b"",
                mode: 0o040755,
                id: 1,
            },
            TestEntry {
                path: "b_file.txt",
                data: b"content",
                mode: 0o100644,
                id: 2,
            },
            TestEntry {
                path: "c_link",
                data: b"target",
                mode: 0o120777,
                id: 3,
            },
        ]);
        let archive = archive_from_bytes(&bytes);

        assert!(archive.entry("a_dir").unwrap().is_directory());
        assert!(archive.entry("b_file.txt").unwrap().is_file());
        assert!(archive.entry("c_link").unwrap().is_symlink());
    }

    /// entry() on nonexistent path returns EntryNotFound.
    #[test]
    fn entry_not_found() {
        let bytes = build_test_archive(&[]);
        let archive = archive_from_bytes(&bytes);

        let result = archive.entry("missing");
        assert!(matches!(result, Err(BaleError::EntryNotFound(_))));
    }

    /// find_by_id finds entries by their stable ID.
    #[test]
    fn find_by_id_works() {
        let bytes = build_test_archive(&[
            TestEntry {
                path: "alpha",
                data: b"a",
                mode: 0o100644,
                id: 10,
            },
            TestEntry {
                path: "beta",
                data: b"b",
                mode: 0o100644,
                id: 20,
            },
        ]);
        let archive = archive_from_bytes(&bytes);

        let entry = archive.find_by_id(10).unwrap();
        assert!(entry.is_file());
        let file = entry.into_file().unwrap();
        assert_eq!(file.path().as_str(), Some("alpha"));
        assert_eq!(file.data(), b"a");

        let entry = archive.find_by_id(20).unwrap();
        let file = entry.into_file().unwrap();
        assert_eq!(file.path().as_str(), Some("beta"));

        assert!(archive.find_by_id(99).is_none());
    }

    /// find_by_id returns directory entries correctly.
    #[test]
    fn find_by_id_directory() {
        let bytes = build_test_archive(&[TestEntry {
            path: "mydir",
            data: b"",
            mode: 0o040755,
            id: 3,
        }]);
        let archive = archive_from_bytes(&bytes);

        let entry = archive.find_by_id(3).unwrap();
        assert!(entry.is_directory());
    }

    /// read_data returns empty slice for directory entries.
    #[test]
    fn read_data_empty_for_directory() {
        let bytes = build_test_archive(&[TestEntry {
            path: "dir",
            data: b"",
            mode: 0o040755,
            id: 1,
        }]);
        let archive = archive_from_bytes(&bytes);

        let entry = archive.find_entry("dir").unwrap();
        let data = archive.read_data(entry).unwrap();
        assert!(data.is_empty());
    }

    /// Archives with small path sizes work correctly.
    #[test]
    fn small_path_size() {
        let bytes = build_test_archive_with_options(
            &[TestEntry {
                path: "hi",
                data: b"data",
                mode: 0o100644,
                id: 1,
            }],
            16,
            4096,
        );
        let archive = archive_from_bytes(&bytes);

        assert_eq!(archive.path_size(), 16);
        let entry = archive.find_entry("hi").unwrap();
        let data = archive.read_data(entry).unwrap();
        assert_eq!(data, b"data");
    }

    // ==================== resolve() Tests ====================

    /// resolve() returns a non-symlink entry directly (fast path).
    #[test]
    fn resolve_direct_file() {
        let bytes = build_test_archive(&[TestEntry {
            path: "hello.txt",
            data: b"Hello",
            mode: 0o100644,
            id: 1,
        }]);
        let archive = archive_from_bytes(&bytes);

        let entry = archive.resolve("hello.txt").unwrap();
        assert!(entry.is_file());
        let file = entry.into_file().unwrap();
        assert_eq!(file.data(), b"Hello");
    }

    /// resolve() follows a simple symlink to a file.
    #[test]
    fn resolve_simple_symlink() {
        let bytes = build_test_archive(&[
            TestEntry {
                path: "link",
                data: b"target.txt",
                mode: 0o120777,
                id: 1,
            },
            TestEntry {
                path: "target.txt",
                data: b"content",
                mode: 0o100644,
                id: 2,
            },
        ]);
        let archive = archive_from_bytes(&bytes);

        let entry = archive.resolve("link").unwrap();
        assert!(entry.is_file());
        let file = entry.into_file().unwrap();
        assert_eq!(file.data(), b"content");
    }

    /// resolve() follows a chain of symlinks (symlink → symlink → file).
    #[test]
    fn resolve_symlink_chain() {
        let bytes = build_test_archive(&[
            TestEntry {
                path: "a",
                data: b"b",
                mode: 0o120777,
                id: 1,
            },
            TestEntry {
                path: "b",
                data: b"c",
                mode: 0o120777,
                id: 2,
            },
            TestEntry {
                path: "c",
                data: b"final",
                mode: 0o100644,
                id: 3,
            },
        ]);
        let archive = archive_from_bytes(&bytes);

        let entry = archive.resolve("a").unwrap();
        assert!(entry.is_file());
        let file = entry.into_file().unwrap();
        assert_eq!(file.data(), b"final");
    }

    /// resolve() follows an intermediate directory symlink.
    #[test]
    fn resolve_intermediate_symlink() {
        let bytes = build_test_archive(&[
            TestEntry {
                path: "dir",
                data: b"",
                mode: 0o040755,
                id: 1,
            },
            TestEntry {
                path: "dir/file.txt",
                data: b"in dir",
                mode: 0o100644,
                id: 2,
            },
            TestEntry {
                path: "link",
                data: b"dir",
                mode: 0o120777,
                id: 3,
            },
        ]);
        let archive = archive_from_bytes(&bytes);

        let entry = archive.resolve("link/file.txt").unwrap();
        assert!(entry.is_file());
        let file = entry.into_file().unwrap();
        assert_eq!(file.data(), b"in dir");
    }

    /// resolve() handles symlink targets containing `..`.
    #[test]
    fn resolve_symlink_with_dotdot() {
        let bytes = build_test_archive(&[
            TestEntry {
                path: "a",
                data: b"",
                mode: 0o040755,
                id: 1,
            },
            TestEntry {
                path: "a/link",
                data: b"../b/target.txt",
                mode: 0o120777,
                id: 2,
            },
            TestEntry {
                path: "b",
                data: b"",
                mode: 0o040755,
                id: 3,
            },
            TestEntry {
                path: "b/target.txt",
                data: b"found it",
                mode: 0o100644,
                id: 4,
            },
        ]);
        let archive = archive_from_bytes(&bytes);

        let entry = archive.resolve("a/link").unwrap();
        assert!(entry.is_file());
        let file = entry.into_file().unwrap();
        assert_eq!(file.data(), b"found it");
    }

    /// resolve() detects circular symlinks and returns SymlinkLoop.
    #[test]
    fn resolve_symlink_loop() {
        let bytes = build_test_archive(&[
            TestEntry {
                path: "x",
                data: b"y",
                mode: 0o120777,
                id: 1,
            },
            TestEntry {
                path: "y",
                data: b"x",
                mode: 0o120777,
                id: 2,
            },
        ]);
        let archive = archive_from_bytes(&bytes);

        let result = archive.resolve("x");
        assert!(matches!(result, Err(BaleError::SymlinkLoop(_))));
    }

    /// resolve() returns EntryNotFound for a dangling symlink.
    #[test]
    fn resolve_dangling_symlink() {
        let bytes = build_test_archive(&[TestEntry {
            path: "broken",
            data: b"nonexistent",
            mode: 0o120777,
            id: 1,
        }]);
        let archive = archive_from_bytes(&bytes);

        let result = archive.resolve("broken");
        assert!(matches!(result, Err(BaleError::EntryNotFound(_))));
    }

    /// resolve() rejects symlinks with absolute targets in malicious archives.
    #[test]
    fn resolve_absolute_symlink_target() {
        let bytes = build_test_archive(&[TestEntry {
            path: "link",
            data: b"/usr/share/data.txt",
            mode: 0o120777,
            id: 1,
        }]);
        let archive = archive_from_bytes(&bytes);

        let result = archive.resolve("link");
        assert!(matches!(result, Err(BaleError::InvalidPath(_))));
    }

    /// resolve() returns NotADirectory when a file is used as a directory.
    #[test]
    fn resolve_file_as_directory() {
        let bytes = build_test_archive(&[TestEntry {
            path: "file.txt",
            data: b"data",
            mode: 0o100644,
            id: 1,
        }]);
        let archive = archive_from_bytes(&bytes);

        let result = archive.resolve("file.txt/child");
        assert!(matches!(result, Err(BaleError::NotADirectory(_))));
    }

    /// resolve() succeeds at exactly MAX_SYMLINK_DEPTH (256) hops.
    #[test]
    fn resolve_symlink_chain_at_max_depth() {
        // Build 256 symlinks: s001 → s002 → ... → s256 → target.txt
        let mut entries: Vec<TestEntry> = (1..=256u32)
            .map(|i| {
                let path: &'static str = Box::leak(format!("s{i:03}").into_boxed_str());
                let target: &'static [u8] = if i < 256 {
                    Box::leak(format!("s{:03}", i + 1).into_bytes().into_boxed_slice())
                } else {
                    b"target.txt"
                };
                TestEntry {
                    path,
                    data: target,
                    mode: 0o120777,
                    id: i,
                }
            })
            .collect();
        entries.push(TestEntry {
            path: "target.txt",
            data: b"reached",
            mode: 0o100644,
            id: 257,
        });

        let bytes = build_test_archive(&entries);
        let archive = archive_from_bytes(&bytes);

        let entry = archive.resolve("s001").unwrap();
        assert!(entry.is_file());
        let file = entry.into_file().unwrap();
        assert_eq!(file.data(), b"reached");
    }

    /// resolve() returns SymlinkLoop when chain exceeds MAX_SYMLINK_DEPTH.
    #[test]
    fn resolve_symlink_chain_exceeds_max_depth() {
        // Build 257 symlinks: s001 → s002 → ... → s257 → target.txt
        let mut entries: Vec<TestEntry> = (1..=257u32)
            .map(|i| {
                let path: &'static str = Box::leak(format!("s{i:03}").into_boxed_str());
                let target: &'static [u8] = if i < 257 {
                    Box::leak(format!("s{:03}", i + 1).into_bytes().into_boxed_slice())
                } else {
                    b"target.txt"
                };
                TestEntry {
                    path,
                    data: target,
                    mode: 0o120777,
                    id: i,
                }
            })
            .collect();
        entries.push(TestEntry {
            path: "target.txt",
            data: b"unreachable",
            mode: 0o100644,
            id: 258,
        });

        let bytes = build_test_archive(&entries);
        let archive = archive_from_bytes(&bytes);

        let result = archive.resolve("s001");
        assert!(matches!(result, Err(BaleError::SymlinkLoop(_))));
    }

    /// resolve() rejects backslash-prefixed absolute symlink targets.
    #[test]
    fn resolve_backslash_absolute_symlink_target() {
        let bytes = build_test_archive(&[TestEntry {
            path: "link",
            data: b"\\foo\\bar",
            mode: 0o120777,
            id: 1,
        }]);
        let archive = archive_from_bytes(&bytes);

        let result = archive.resolve("link");
        assert!(matches!(result, Err(BaleError::InvalidPath(_))));
    }

    /// resolve() rejects root-escaping symlink targets like `../../outside.txt`.
    #[test]
    fn resolve_root_escaping_symlink_target() {
        let bytes = build_test_archive(&[TestEntry {
            path: "link",
            data: b"../../outside.txt",
            mode: 0o120777,
            id: 1,
        }]);
        let archive = archive_from_bytes(&bytes);

        let result = archive.resolve("link");
        assert!(matches!(result, Err(BaleError::InvalidPath(_))));
    }
}
