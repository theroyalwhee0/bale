//! Read-only archive implementation.
//!
//! **Stub**: This implementation is being rewritten for the v1.0.0 format.
//! The reader will be implemented in issue #128.

use super::{Archive, ArchiveRead, DirEntry, Entry, FileEntry, SymlinkEntry};
use crate::format::{EntryRow, Trailer};
use crate::{ArchivePath, BaleError, MappedArchive};
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

        Ok(Self {
            mmap,
            trailer,
            write_offset: 0,
            dirty: false,
        })
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
    /// # Panics
    ///
    /// Panics if alignment_power is invalid (cannot happen for validated archives).
    fn alignment(&self) -> u32 {
        self.trailer.alignment().expect("validated on construction")
    }

    /// Returns the path at the given directory table index.
    fn get_path(&self, _index: usize) -> Option<ArchivePath<'_>> {
        todo!("will be implemented in #128")
    }

    /// Iterates over all entry rows with their paths.
    fn iter_entries(&self) -> impl Iterator<Item = (&EntryRow, &[u8])> {
        std::iter::empty()
    }

    /// Finds an entry by path.
    fn find_entry(&self, _path: &str) -> Option<&EntryRow> {
        todo!("will be implemented in #128")
    }

    /// Finds an entry by path with path bytes and ID.
    fn find_entry_with_path(&self, _path: &str) -> Option<(&EntryRow, &[u8], u32)> {
        todo!("will be implemented in #128")
    }

    /// Reads data for the given entry row.
    ///
    /// # Errors
    ///
    /// Returns an error if the data offset or size is invalid.
    fn read_data(&self, _entry: &EntryRow) -> Result<&[u8], BaleError> {
        todo!("will be implemented in #128")
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
        todo!("will be implemented in #128")
    }

    /// Checks if the directory table is sorted.
    fn is_sorted(&self) -> bool {
        todo!("will be implemented in #128")
    }

    /// Returns duplicate paths.
    fn find_duplicates(&self) -> Vec<ArchivePath<'static>> {
        todo!("will be implemented in #128")
    }

    /// Checks for orphaned data blocks.
    fn has_orphaned_data(&self) -> bool {
        todo!("will be implemented in #128")
    }

    /// Returns a file entry by path.
    ///
    /// # Errors
    ///
    /// Returns an error if not found or not a file.
    fn file(&self, _path: impl AsRef<str>) -> Result<FileEntry<'_>, BaleError> {
        todo!("will be implemented in #128")
    }

    /// Returns a directory entry by path.
    ///
    /// # Errors
    ///
    /// Returns an error if not found or not a directory.
    fn folder(&self, _path: impl AsRef<str>) -> Result<DirEntry<'_>, BaleError> {
        todo!("will be implemented in #128")
    }

    /// Returns a symlink entry by path.
    ///
    /// # Errors
    ///
    /// Returns an error if not found or not a symlink.
    fn symlink(&self, _path: impl AsRef<str>) -> Result<SymlinkEntry<'_>, BaleError> {
        todo!("will be implemented in #128")
    }

    /// Returns any entry by path.
    ///
    /// # Errors
    ///
    /// Returns an error if not found.
    fn entry(&self, _path: impl AsRef<str>) -> Result<Entry<'_>, BaleError> {
        todo!("will be implemented in #128")
    }

    /// Finds an entry by its stable ID.
    fn find_by_id(&self, _id: u32) -> Option<Entry<'_>> {
        todo!("will be implemented in #128")
    }
}
