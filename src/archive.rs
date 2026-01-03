//! Unified archive access using memory-mapped I/O.
//!
//! This module provides a generic `Archive<M>` struct that works with both
//! read-only and read-write memory maps. The `ArchiveRead` and `ArchiveWrite`
//! traits define the available operations for each mode.
//!
//! # Type Aliases
//!
//! - [`ArchiveReader`] = `Archive<MappedArchive>` - Read-only access
//! - [`ArchiveWriter`] = `Archive<MappedArchiveMut>` - Read-write access

use crate::{
    ArchivePath, BaleEocd, BaleError, CentralDirectoryHeader, DosDateTime, Eocd, LocalFileHeader,
    MappedArchive, MappedArchiveMut, Trailer, Zip64Eocd,
};
use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use zerocopy::{FromBytes, IntoBytes};

/// Pre-allocated zero buffer for padding (avoids allocation for common cases).
///
/// Sized to match the default alignment (4096 bytes). For larger alignments,
/// padding is written in chunks from this buffer.
static ZERO_PAD: [u8; 4096] = [0u8; 4096];

/// An in-memory Central Directory entry.
///
/// Holds the header data and null-padded path for an entry.
struct CdEntry {
    /// The Central Directory header.
    header: CentralDirectoryHeader,
    /// The null-padded path (length = path_size).
    path: Vec<u8>,
}

/// A bale archive with generic memory-map backing.
///
/// This struct provides access to archive contents through memory-mapped I/O.
/// The type parameter `M` determines whether the archive is read-only
/// ([`MappedArchive`]) or read-write ([`MappedArchiveMut`]).
///
/// # Example
///
/// ```ignore
/// use bale::{ArchiveReader, ArchiveRead};
///
/// let reader = ArchiveReader::open("archive.bale")?;
/// for (header, path) in reader.iter_entries() {
///     let data = reader.read_data(header)?;
///     // process data...
/// }
/// ```
pub struct Archive<M> {
    /// The memory-mapped archive file.
    mmap: M,
    /// In-memory Central Directory entries.
    entries: Vec<CdEntry>,
    /// Archive configuration from the BaleEocd.
    bale_eocd: BaleEocd,
    /// Current write offset (end of file data). Only used for writers.
    write_offset: usize,
    /// Whether the archive has been modified since the last sync. Only used for writers.
    dirty: bool,
}

/// Read-only archive type alias.
pub type ArchiveReader = Archive<MappedArchive>;

/// Read-write archive type alias.
pub type ArchiveWriter = Archive<MappedArchiveMut>;

/// Read operations for archives.
///
/// This trait is implemented for all `Archive<M>` where `M` provides byte access.
pub trait ArchiveRead {
    /// Returns the number of entries in the archive.
    fn entry_count(&self) -> usize;

    /// Returns the configured path size for this archive.
    fn path_size(&self) -> usize;

    /// Returns the configured alignment for this archive.
    ///
    /// # Panics
    ///
    /// Panics if `alignment_pow2` is invalid. This cannot happen for archives
    /// opened via [`open()`](Archive::open) since validation occurs on construction.
    fn alignment(&self) -> u32;

    /// Returns the path for the entry at the given index as a zero-copy `ArchivePath`.
    ///
    /// The returned path borrows directly from the in-memory entries.
    /// Returns `None` if the index is out of bounds.
    fn get_path(&self, index: usize) -> Option<ArchivePath<'_>>;

    /// Returns an iterator over all Central Directory entries.
    ///
    /// Each item is a tuple of (header, path_bytes) where path_bytes is the
    /// null-padded path from the CD entry.
    fn iter_entries(&self) -> impl Iterator<Item = (&CentralDirectoryHeader, &[u8])>;

    /// Finds an entry by path using linear scan.
    ///
    /// Returns the last matching entry. Bale uses append-only shadowing: when
    /// a file is updated, the new version is appended and the old version
    /// remains but is "shadowed". The last occurrence is the current version.
    ///
    /// The path comparison is byte-exact against the null-padded path.
    fn find_entry(&self, path: &str) -> Option<&CentralDirectoryHeader>;

    /// Returns a zero-copy slice of the file data for the given entry.
    ///
    /// # Errors
    ///
    /// Returns an error if the entry's offset or size is invalid.
    fn read_data(&self, entry: &CentralDirectoryHeader) -> Result<&[u8], BaleError>;

    /// Returns a reference to the BaleEocd.
    fn bale_eocd(&self) -> &BaleEocd;

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
    fn verify_crc(&self, entry: &CentralDirectoryHeader) -> Result<(), BaleError>;

    /// Checks if the Central Directory is sorted by path bytes.
    ///
    /// A sorted CD enables binary search for entry lookup. Archives created
    /// by `compact` are always sorted.
    fn is_sorted(&self) -> bool;

    /// Returns a list of duplicate paths in the archive.
    ///
    /// Duplicate paths occur when the same path appears multiple times in the
    /// Central Directory (shadowing). Returns the paths that have duplicates,
    /// not the total count of duplicates.
    fn find_duplicates(&self) -> Vec<String>;

    /// Checks if the archive contains orphaned data.
    ///
    /// Orphaned data exists when there are gaps between entries or between
    /// the last entry and the Central Directory. This can occur after
    /// deletions or when entries are shadowed.
    fn has_orphaned_data(&self) -> bool;
}

/// Write operations for archives.
///
/// This trait is only implemented for `Archive<MappedArchiveMut>`.
pub trait ArchiveWrite: ArchiveRead {
    /// Adds an entry from raw data.
    ///
    /// If an entry with the same path already exists, the new entry shadows it.
    /// The old data remains in the archive (orphaned) until compact.
    ///
    /// # Arguments
    ///
    /// * `path` - Archive path for the entry
    /// * `data` - File contents
    /// * `mode` - Unix file permissions (e.g., 0o644)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The path exceeds the archive's path_size
    /// - The data size exceeds 4GB (ZIP format limitation)
    /// - The archive offset would exceed 4GB (ZIP format limitation)
    /// - Writing to the archive fails
    fn add_entry(&mut self, path: &str, data: &[u8], mode: u32) -> Result<(), BaleError>;

    /// Adds a file from the filesystem to the archive.
    ///
    /// # Memory usage
    ///
    /// This method reads the entire file into memory before writing to the
    /// archive. For very large files, consider using [`add_entry()`](Self::add_entry)
    /// with a streaming approach, or ensure sufficient memory is available.
    ///
    /// # Arguments
    ///
    /// * `src` - Path to the source file
    /// * `archive_path` - Path within the archive
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The source file cannot be read
    /// - The archive path exceeds path_size
    /// - The file size exceeds 4GB (ZIP format limitation)
    /// - Writing to the archive fails
    fn add_file(&mut self, src: impl AsRef<Path>, archive_path: &str) -> Result<(), BaleError>;

    /// Deletes all entries matching a path.
    ///
    /// Removes all matching entries from the Central Directory. If duplicate
    /// entries exist (from shadowing), all are removed. The file data remains
    /// in the archive (orphaned) until a compact operation.
    ///
    /// Returns `true` if any entries were deleted, `false` if none matched.
    fn delete(&mut self, path: &str) -> bool;

    /// Flushes all changes to disk.
    ///
    /// Rewrites the Central Directory and full trailer (ZIP64 EOCD, ZIP64 EOCD
    /// Locator, EOCD, and BaleEocd). The CD starts at an aligned offset for
    /// efficient mmap access. The file is truncated to the logical size.
    ///
    /// If no changes have been made since the last sync, this is a no-op.
    ///
    /// # Errors
    ///
    /// Returns an error if writing or syncing fails.
    fn sync(&mut self) -> Result<(), BaleError>;
}

// ============================================================================
// Shared implementation for parsing archives
// ============================================================================

/// Parses Central Directory entries from archive bytes.
///
/// # Arguments
///
/// * `bytes` - The full archive bytes
/// * `cd_offset` - Offset to the start of the Central Directory
/// * `entry_count` - Number of entries in the CD
/// * `path_size` - Size of each path field
///
/// # Errors
///
/// Returns an error if any CD entry is malformed or extends beyond the archive.
fn parse_cd_entries(
    bytes: &[u8],
    cd_offset: usize,
    entry_count: usize,
    path_size: usize,
) -> Result<Vec<CdEntry>, BaleError> {
    let stride = CentralDirectoryHeader::stride(path_size);
    let mut entries = Vec::with_capacity(entry_count);

    for i in 0..entry_count {
        let entry_start = cd_offset + i * stride;
        let entry_end = entry_start + stride;
        if entry_end > bytes.len() {
            return Err(BaleError::Corrupted(format!(
                "CD entry {i} extends beyond archive"
            )));
        }

        let entry_bytes = &bytes[entry_start..entry_end];
        let header =
            CentralDirectoryHeader::ref_from_bytes(&entry_bytes[..CentralDirectoryHeader::SIZE])
                .map_err(|e| BaleError::Corrupted(format!("invalid CD entry {i}: {e}")))?;

        entries.push(CdEntry {
            header: *header,
            path: entry_bytes[CentralDirectoryHeader::SIZE..].to_vec(),
        });
    }

    Ok(entries)
}

// ============================================================================
// ArchiveReader implementation
// ============================================================================

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

// ============================================================================
// ArchiveWriter implementation
// ============================================================================

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

// ============================================================================
// Helper functions
// ============================================================================

/// Converts a null-padded path to a string.
fn path_to_string(path_bytes: &[u8]) -> String {
    let end = path_bytes
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(path_bytes.len());
    String::from_utf8_lossy(&path_bytes[..end]).to_string()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
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
