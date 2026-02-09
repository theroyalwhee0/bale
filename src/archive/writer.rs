//! Read-write archive implementation.
//!
//! **Stub**: This implementation is being rewritten for the v1.0.0 format.
//! The writer will be implemented in issue #129.

use super::{Archive, ArchiveRead, ArchiveWrite, DirEntry, Entry, FileEntry, SymlinkEntry};
use crate::format::{EntryRow, Trailer};
use crate::{ArchivePath, BaleError, MappedArchiveMut};
use std::path::Path;
use zerocopy::FromBytes;

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
        let mmap = MappedArchiveMut::create(path)?;

        Ok(Self {
            mmap,
            trailer,
            write_offset: 0,
            dirty: true, // New archive needs initial sync to write header + trailer.
        })
    }

    /// Opens an existing archive for appending.
    ///
    /// Reads the trailer to determine table locations and configuration.
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

        // The write offset is where new data blocks should go — at the entry
        // table offset (since we'll rewrite tables on sync).
        let write_offset = trailer.entry_table_offset.get() as usize;

        Ok(Self {
            mmap,
            trailer,
            write_offset,
            dirty: false,
        })
    }
}

impl ArchiveRead for Archive<MappedArchiveMut> {
    /// Returns the number of entries.
    fn entry_count(&self) -> usize {
        self.trailer.entry_count.get() as usize
    }

    /// Returns the configured path size.
    fn path_size(&self) -> usize {
        self.trailer.path_size() as usize
    }

    /// Returns the configured alignment.
    ///
    /// # Panics
    ///
    /// Panics if alignment_power is invalid (cannot happen for validated archives).
    fn alignment(&self) -> u32 {
        self.trailer.alignment().expect("validated on construction")
    }

    /// Returns the path at the given directory table index.
    fn get_path(&self, _index: usize) -> Option<ArchivePath<'_>> {
        todo!("will be implemented in #129")
    }

    /// Iterates over all entry rows with their paths.
    fn iter_entries(&self) -> impl Iterator<Item = (&EntryRow, &[u8])> {
        std::iter::empty()
    }

    /// Finds an entry by path.
    fn find_entry(&self, _path: &str) -> Option<&EntryRow> {
        todo!("will be implemented in #129")
    }

    /// Finds an entry by path with path bytes and ID.
    fn find_entry_with_path(&self, _path: &str) -> Option<(&EntryRow, &[u8], u32)> {
        todo!("will be implemented in #129")
    }

    /// Reads data for the given entry row.
    ///
    /// # Errors
    ///
    /// Returns an error if the data offset or size is invalid.
    fn read_data(&self, _entry: &EntryRow) -> Result<&[u8], BaleError> {
        todo!("will be implemented in #129")
    }

    /// Returns a reference to the trailer.
    fn trailer(&self) -> &Trailer {
        &self.trailer
    }

    /// Verifies the CRC-32 checksum for an entry.
    ///
    /// # Errors
    ///
    /// Returns an error if the CRC does not match.
    fn verify_crc(&self, _entry: &EntryRow) -> Result<(), BaleError> {
        todo!("will be implemented in #129")
    }

    /// Checks if the directory table is sorted.
    fn is_sorted(&self) -> bool {
        todo!("will be implemented in #129")
    }

    /// Returns duplicate paths.
    fn find_duplicates(&self) -> Vec<ArchivePath<'static>> {
        todo!("will be implemented in #129")
    }

    /// Checks for orphaned data blocks.
    fn has_orphaned_data(&self) -> bool {
        todo!("will be implemented in #129")
    }

    /// Returns a file entry by path.
    ///
    /// # Errors
    ///
    /// Returns an error if not found or not a file.
    fn file(&self, _path: impl AsRef<str>) -> Result<FileEntry<'_>, BaleError> {
        todo!("will be implemented in #129")
    }

    /// Returns a directory entry by path.
    ///
    /// # Errors
    ///
    /// Returns an error if not found or not a directory.
    fn folder(&self, _path: impl AsRef<str>) -> Result<DirEntry<'_>, BaleError> {
        todo!("will be implemented in #129")
    }

    /// Returns a symlink entry by path.
    ///
    /// # Errors
    ///
    /// Returns an error if not found or not a symlink.
    fn symlink(&self, _path: impl AsRef<str>) -> Result<SymlinkEntry<'_>, BaleError> {
        todo!("will be implemented in #129")
    }

    /// Returns any entry by path.
    ///
    /// # Errors
    ///
    /// Returns an error if not found.
    fn entry(&self, _path: impl AsRef<str>) -> Result<Entry<'_>, BaleError> {
        todo!("will be implemented in #129")
    }

    /// Finds an entry by its stable ID.
    fn find_by_id(&self, _id: u32) -> Option<Entry<'_>> {
        todo!("will be implemented in #129")
    }
}

impl ArchiveWrite for Archive<MappedArchiveMut> {
    /// Adds an entry from raw data.
    ///
    /// # Errors
    ///
    /// Returns an error if the path is invalid or writing fails.
    fn add_entry(&mut self, _path: &str, _data: &[u8], _mode: u32) -> Result<(), BaleError> {
        todo!("will be implemented in #129")
    }

    /// Adds an entry with a specific modification time.
    ///
    /// # Errors
    ///
    /// Returns an error if the path is invalid or writing fails.
    fn add_entry_with_mtime(
        &mut self,
        _path: &str,
        _data: &[u8],
        _mode: u32,
        _mtime: Option<std::time::SystemTime>,
    ) -> Result<(), BaleError> {
        todo!("will be implemented in #129")
    }

    /// Adds a file from the filesystem.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or writing fails.
    fn add_file(&mut self, _src: impl AsRef<Path>, _archive_path: &str) -> Result<(), BaleError> {
        todo!("will be implemented in #129")
    }

    /// Deletes all entries matching a path.
    fn delete(&mut self, _path: &str) -> bool {
        todo!("will be implemented in #129")
    }

    /// Flushes all changes to disk.
    ///
    /// # Errors
    ///
    /// Returns an error if writing or syncing fails.
    fn sync(&mut self) -> Result<(), BaleError> {
        todo!("will be implemented in #129")
    }

    /// Creates a directory entry.
    ///
    /// # Errors
    ///
    /// Returns an error if the path is invalid or writing fails.
    fn add_folder(&mut self, _path: impl AsRef<str>, _mode: u32) -> Result<(), BaleError> {
        todo!("will be implemented in #129")
    }

    /// Creates a symlink entry.
    ///
    /// # Errors
    ///
    /// Returns an error if the path is invalid or writing fails.
    fn add_symlink(
        &mut self,
        _path: impl AsRef<str>,
        _target: impl AsRef<str>,
        _mode: u32,
    ) -> Result<(), BaleError> {
        todo!("will be implemented in #129")
    }
}
