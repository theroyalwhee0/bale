//! Internal state for the BaleFs filesystem.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use fuser::FileType;
use nix::libc;
use nix::sys::stat::{Mode, SFlag};

use crate::format::EntryRow;
use crate::fuse::virtual_dir::VirtualDir;
use crate::fuse::{DIR_INO_START, FILE_INO_START, FuseDirEntry, ROOT_INO};
use crate::{ArchivePath, ArchiveRead, ArchiveWrite, ArchiveWriter, BaleError, EntryKind};

/// Default permission bits for regular files (rw-r--r--).
pub(super) const DEFAULT_FILE_PERM: Mode = Mode::from_bits_truncate(0o644);

/// Default permission bits for directories (rwxr-xr-x).
pub(super) const DEFAULT_DIR_PERM: Mode = Mode::from_bits_truncate(0o755);

/// Default file mode: regular file with default permissions.
const DEFAULT_FILE_MODE: u32 = SFlag::S_IFREG.bits() | DEFAULT_FILE_PERM.bits();

/// Default directory mode: directory with default permissions.
const DEFAULT_DIR_MODE: u32 = SFlag::S_IFDIR.bits() | DEFAULT_DIR_PERM.bits();

/// Internal state for the BaleFs filesystem.
pub(super) struct BaleFsState {
    /// The underlying archive.
    pub(super) archive: ArchiveWriter,
    /// Whether the filesystem is mounted read-only.
    pub(super) read_only: bool,
    /// User ID for all files/directories.
    pub(super) uid: u32,
    /// Group ID for all files/directories.
    pub(super) gid: u32,
    /// Time when the filesystem was mounted.
    pub(super) mount_time: SystemTime,

    /// Map from directory path to its inode.
    pub(super) dir_inodes: HashMap<String, u64>,
    /// Map from directory inode to its contents.
    pub(super) dir_contents: HashMap<u64, Vec<FuseDirEntry>>,

    /// Map from file/symlink inode to archive path.
    pub(super) inode_to_path: HashMap<u64, String>,
    /// Map from archive path to file/symlink inode.
    pub(super) path_to_inode: HashMap<String, u64>,

    /// Modified data for files (path -> new content).
    /// Used for write support when not read-only.
    pub(super) modified_data: HashMap<String, Vec<u8>>,

    /// Modified permission bits for files (path -> new perms).
    /// Tracks chmod operations until synced to archive.
    pub(super) modified_modes: HashMap<String, Mode>,

    /// Modified timestamps for files (path -> new mtime).
    /// Tracks utimensat operations until synced to archive.
    pub(super) modified_times: HashMap<String, SystemTime>,

    /// Transient uid overrides (path -> uid). Session-only, not persisted.
    pub(super) modified_uids: HashMap<String, u32>,
    /// Transient gid overrides (path -> gid). Session-only, not persisted.
    pub(super) modified_gids: HashMap<String, u32>,

    /// Directory mtimes by inode.
    /// For explicit directories, this is from the archive.
    /// For derived directories, this is the max mtime of contained files.
    dir_mtimes: HashMap<u64, SystemTime>,

    /// Map from archive entry ID to file inode.
    /// Used to share inodes across hard-linked paths.
    entry_id_to_ino: HashMap<u32, u64>,

    /// Hard link count per inode.
    /// Tracks how many paths point to the same underlying entry.
    pub(super) nlink_counts: HashMap<u64, u32>,

    /// Mount point for symlink target normalization.
    /// Set at mount time, used to strip absolute targets to archive-relative paths.
    mount_point: Option<PathBuf>,

    /// Next available inode for files.
    next_file_ino: u64,
    /// Next available inode for directories.
    next_dir_ino: u64,

    /// Virtual `.bale/` metadata directory.
    pub(super) virtual_dir: VirtualDir,
}

impl BaleFsState {
    /// Creates a new BaleFsState with the given archive.
    pub(super) fn new(archive: ArchiveWriter, read_only: bool, uid: u32, gid: u32) -> Self {
        let mut state = Self {
            archive,
            read_only,
            uid,
            gid,
            mount_time: SystemTime::now(),
            dir_inodes: HashMap::new(),
            dir_contents: HashMap::new(),
            inode_to_path: HashMap::new(),
            path_to_inode: HashMap::new(),
            modified_data: HashMap::new(),
            modified_modes: HashMap::new(),
            modified_times: HashMap::new(),
            modified_uids: HashMap::new(),
            modified_gids: HashMap::new(),
            dir_mtimes: HashMap::new(),
            entry_id_to_ino: HashMap::new(),
            nlink_counts: HashMap::new(),
            mount_point: None,
            next_file_ino: FILE_INO_START,
            next_dir_ino: DIR_INO_START,
            virtual_dir: VirtualDir::new(),
        };
        state.build_directory_tree();
        state
    }

    /// Sets the mount point for symlink target normalization.
    pub(super) fn set_mount_point(&mut self, path: PathBuf) {
        self.mount_point = Some(path);
    }

    /// Validates a filename component using safename rules.
    ///
    /// Checks length (≤255 bytes) and security rules (no control chars,
    /// leading dashes, etc.).
    ///
    /// Returns `Ok(())` if valid, or `Err(errno)` on failure.
    pub(super) fn validate_name(name: &str) -> Result<(), i32> {
        safename::validate_file(name).map_err(|e| match e {
            safename::SafeNameError::InvalidLength { .. } => libc::ENAMETOOLONG,
            safename::SafeNameError::InvalidByte { .. } => libc::EINVAL,
        })
    }

    /// Validates that a full path doesn't exceed the archive's path_size.
    ///
    /// Returns `Ok(())` if valid, or `Err(ENAMETOOLONG)` if too long.
    pub(super) fn validate_path_length(&self, path: &str) -> Result<(), i32> {
        if path.len() > self.archive.path_size() {
            Err(libc::ENAMETOOLONG)
        } else {
            Ok(())
        }
    }

    /// Validates a full path using ArchivePath rules (safename + reserved prefix).
    ///
    /// Returns `Ok(())` if valid, or `Err(errno)` on failure.
    fn validate_archive_path(path: &str) -> Result<(), i32> {
        ArchivePath::try_from(path).map_err(|e| match e {
            BaleError::UnsafeFilename(_) => libc::EINVAL,
            BaleError::ReservedPath(_) => libc::EINVAL,
            BaleError::InvalidPath => libc::EINVAL,
            _ => libc::EIO,
        })?;
        Ok(())
    }

    /// Builds the directory tree from archive entries.
    ///
    /// Iterates through all archive entries and creates:
    /// - Directory inodes for all directories (explicit and implicit)
    /// - File/symlink inodes for all files and symlinks
    /// - Directory content listings
    /// - Directory mtimes (from archive for explicit, max child mtime for derived)
    fn build_directory_tree(&mut self) {
        // Root directory always exists.
        self.dir_inodes.insert(String::new(), ROOT_INO);
        self.dir_contents.insert(ROOT_INO, Vec::new());
        self.dir_mtimes.insert(ROOT_INO, self.mount_time);

        // Collect all paths first to avoid borrow issues.
        // Skip entries with invalid UTF-8 paths.
        let entries: Vec<(String, EntryKind, SystemTime, u32)> = self
            .archive
            .iter_entries()
            .filter_map(|(entry_row, path_bytes)| {
                let trimmed = &path_bytes[..path_bytes
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(path_bytes.len())];
                let path = std::str::from_utf8(trimmed).ok()?.to_owned();
                let kind = entry_row.kind();
                let mtime_ms = entry_row.modified_time.get();
                let mtime = SystemTime::UNIX_EPOCH
                    + std::time::Duration::from_millis(mtime_ms.max(0) as u64);
                let entry_id = entry_row.entry_id.get();
                Some((path, kind, mtime, entry_id))
            })
            .collect();

        for (path, kind, mtime, entry_id) in entries {
            // Ensure all parent directories exist.
            self.ensure_parent_dirs(&path);

            match kind {
                EntryKind::Directory => {
                    // Explicit directory entry.
                    let dir_path = path.trim_end_matches('/').to_string();
                    if !self.dir_inodes.contains_key(&dir_path) {
                        let ino = self.next_dir_ino;
                        self.next_dir_ino += 1;
                        self.dir_inodes.insert(dir_path.clone(), ino);
                        self.dir_contents.insert(ino, Vec::new());
                        self.dir_mtimes.insert(ino, mtime);

                        // Add to parent directory.
                        if let Some((parent, name)) = Self::split_path(&dir_path)
                            && let Some(&parent_ino) = self.dir_inodes.get(parent)
                            && let Some(contents) = self.dir_contents.get_mut(&parent_ino)
                        {
                            contents.push(FuseDirEntry::directory(name, ino));
                        }
                    } else {
                        // Directory already exists (was derived), update mtime from explicit entry.
                        if let Some(&ino) = self.dir_inodes.get(&dir_path) {
                            self.dir_mtimes.insert(ino, mtime);
                        }
                    }
                }
                EntryKind::File | EntryKind::Symlink => {
                    // Check if this entry_id was already seen (hard link).
                    let ino = if let Some(&existing_ino) = self.entry_id_to_ino.get(&entry_id) {
                        // Hard link: reuse the existing inode.
                        *self.nlink_counts.entry(existing_ino).or_insert(1) += 1;
                        existing_ino
                    } else {
                        // New entry: allocate a fresh inode.
                        let ino = self.next_file_ino;
                        self.next_file_ino += 1;
                        self.entry_id_to_ino.insert(entry_id, ino);
                        self.nlink_counts.insert(ino, 1);
                        ino
                    };

                    self.inode_to_path.insert(ino, path.clone());
                    self.path_to_inode.insert(path.clone(), ino);

                    // Add to parent directory and update parent's mtime if needed.
                    if let Some((parent, name)) = Self::split_path(&path)
                        && let Some(&parent_ino) = self.dir_inodes.get(parent)
                    {
                        if let Some(contents) = self.dir_contents.get_mut(&parent_ino) {
                            let entry = if kind == EntryKind::Symlink {
                                FuseDirEntry::symlink(name, ino)
                            } else {
                                FuseDirEntry::file(name, ino)
                            };
                            contents.push(entry);
                        }

                        // Update derived directory mtime to max of children.
                        self.dir_mtimes
                            .entry(parent_ino)
                            .and_modify(|t| {
                                if mtime > *t {
                                    *t = mtime;
                                }
                            })
                            .or_insert(mtime);
                    }
                }
                EntryKind::Other(_) => {
                    // Skip unknown entry types.
                }
            }
        }
    }

    /// Ensures all parent directories exist for a given path.
    fn ensure_parent_dirs(&mut self, path: &str) {
        let mut current = String::new();

        for component in path.trim_end_matches('/').split('/') {
            if component.is_empty() {
                continue;
            }

            let parent = current.clone();
            if current.is_empty() {
                current = component.to_string();
            } else {
                current = format!("{current}/{component}");
            }

            // Check if this is the final component (the file/entry itself).
            // Only create directory entries for intermediate components.
            if current == path.trim_end_matches('/') {
                break;
            }

            if !self.dir_inodes.contains_key(&current) {
                let ino = self.next_dir_ino;
                self.next_dir_ino += 1;
                self.dir_inodes.insert(current.clone(), ino);
                self.dir_contents.insert(ino, Vec::new());
                // Derived directory gets mount_time initially; updated to max child mtime later.
                self.dir_mtimes.insert(ino, self.mount_time);

                // Add to parent directory.
                let parent_ino = if parent.is_empty() {
                    ROOT_INO
                } else {
                    *self.dir_inodes.get(&parent).unwrap_or(&ROOT_INO)
                };

                if let Some(contents) = self.dir_contents.get_mut(&parent_ino) {
                    contents.push(FuseDirEntry::directory(component, ino));
                }
            }
        }
    }

    /// Splits a path into parent directory and filename.
    pub(super) fn split_path(path: &str) -> Option<(&str, &str)> {
        let path = path.trim_end_matches('/');
        if let Some(pos) = path.rfind('/') {
            Some((&path[..pos], &path[pos + 1..]))
        } else if !path.is_empty() {
            Some(("", path))
        } else {
            None
        }
    }

    /// Creates file attributes for a directory or generic entry.
    pub(super) fn get_attr(&self, ino: u64, kind: FileType) -> fuser::FileAttr {
        let (perm, mtime) = if kind == FileType::Directory {
            (
                self.get_dir_mode(ino).bits() as u16,
                self.get_dir_mtime(ino),
            )
        } else {
            (DEFAULT_FILE_PERM.bits() as u16, self.mount_time)
        };

        fuser::FileAttr {
            ino,
            size: 0,
            blocks: 0,
            atime: mtime,
            mtime,
            ctime: mtime,
            crtime: mtime,
            kind,
            perm,
            nlink: 1,
            uid: self.get_dir_uid(ino),
            gid: self.get_dir_gid(ino),
            rdev: 0,
            blksize: 4096,
            flags: 0,
        }
    }

    /// Creates file attributes from archive entry row.
    pub(super) fn get_attr_for_file(
        &self,
        ino: u64,
        kind: FileType,
        entry_row: &EntryRow,
        path: &str,
    ) -> fuser::FileAttr {
        let perm = Mode::from_bits_truncate(entry_row.mode.get());
        let size = entry_row.file_size.get();
        let nlink = self.nlink_counts.get(&ino).copied().unwrap_or(1);

        // Check modified_times first, fall back to archive mtime.
        let mtime = self.modified_times.get(path).copied().unwrap_or_else(|| {
            let mtime_ms = entry_row.modified_time.get();
            SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(mtime_ms.max(0) as u64)
        });

        // Use created_time from entry row for crtime and ctime.
        let ctime = {
            let ctime_ms = entry_row.created_time.get();
            SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(ctime_ms.max(0) as u64)
        };

        fuser::FileAttr {
            ino,
            size,
            blocks: size.div_ceil(512),
            atime: mtime,
            mtime,
            ctime,
            crtime: ctime,
            kind,
            perm: if perm.is_empty() {
                DEFAULT_FILE_PERM.bits() as u16
            } else {
                perm.bits() as u16
            },
            nlink,
            uid: self.get_uid(path),
            gid: self.get_gid(path),
            rdev: 0,
            blksize: 4096,
            flags: 0,
        }
    }

    /// Loads a file's content into modified_data if not already present.
    ///
    /// Returns a mutable reference to the file's data buffer.
    pub(super) fn load_into_modified(&mut self, path: &str) -> Result<&mut Vec<u8>, i32> {
        if !self.modified_data.contains_key(path) {
            let data = self
                .archive
                .file(path)
                .map_err(|_| libc::EIO)?
                .data()
                .to_vec();
            self.modified_data.insert(path.to_string(), data);
        }
        Ok(self.modified_data.get_mut(path).unwrap())
    }

    /// Gets the permission bits for a file.
    ///
    /// Checks modified_modes first, then falls back to archive.
    pub(super) fn get_file_mode(&self, path: &str) -> Mode {
        // Check if mode was modified via chmod.
        if let Some(&mode) = self.modified_modes.get(path) {
            return mode;
        }

        // Fall back to archive (type bits are automatically dropped).
        self.archive
            .find_entry_with_path(path)
            .map(|(entry_row, _, _)| {
                let mode = Mode::from_bits_truncate(entry_row.mode.get());
                if mode.is_empty() {
                    DEFAULT_FILE_PERM
                } else {
                    mode
                }
            })
            .unwrap_or(DEFAULT_FILE_PERM)
    }

    /// Gets the permission bits for a directory by inode.
    ///
    /// Checks modified_modes first, then falls back to archive or default.
    pub(super) fn get_dir_mode(&self, ino: u64) -> Mode {
        let path = self.get_dir_path(ino);

        // Check if mode was modified via chmod.
        if let Some(&mode) = self.modified_modes.get(&path) {
            return mode;
        }

        // Check archive for explicit directory entry (type bits are automatically dropped).
        if !path.is_empty()
            && let Some((entry_row, _, _)) = self.archive.find_entry_with_path(&path)
        {
            let mode = Mode::from_bits_truncate(entry_row.mode.get());
            if !mode.is_empty() {
                return mode;
            }
        }

        DEFAULT_DIR_PERM
    }

    /// Sets the permission bits for a directory by inode.
    pub(super) fn set_dir_mode(&mut self, ino: u64, mode: Mode) {
        let path = self.get_dir_path(ino);
        self.modified_modes.insert(path, mode);
    }

    /// Gets the uid for a file or symlink by path.
    ///
    /// Checks transient overrides first, then falls back to mounting user.
    pub(super) fn get_uid(&self, path: &str) -> u32 {
        self.modified_uids.get(path).copied().unwrap_or(self.uid)
    }

    /// Gets the gid for a file or symlink by path.
    ///
    /// Checks transient overrides first, then falls back to mounting group.
    pub(super) fn get_gid(&self, path: &str) -> u32 {
        self.modified_gids.get(path).copied().unwrap_or(self.gid)
    }

    /// Gets the uid for a directory by inode.
    ///
    /// Checks transient overrides first, then falls back to mounting user.
    pub(super) fn get_dir_uid(&self, ino: u64) -> u32 {
        let path = self.get_dir_path(ino);
        self.modified_uids.get(&path).copied().unwrap_or(self.uid)
    }

    /// Gets the gid for a directory by inode.
    ///
    /// Checks transient overrides first, then falls back to mounting group.
    pub(super) fn get_dir_gid(&self, ino: u64) -> u32 {
        let path = self.get_dir_path(ino);
        self.modified_gids.get(&path).copied().unwrap_or(self.gid)
    }

    /// Gets the mtime for a directory by inode.
    ///
    /// Checks modified_times first, then falls back to cached dir_mtimes.
    pub(super) fn get_dir_mtime(&self, ino: u64) -> SystemTime {
        let path = self.get_dir_path(ino);

        // Check if mtime was modified via utimensat.
        if let Some(&mtime) = self.modified_times.get(&path) {
            return mtime;
        }

        // Fall back to cached directory mtime.
        self.dir_mtimes
            .get(&ino)
            .copied()
            .unwrap_or(self.mount_time)
    }

    /// Sets the mtime for a directory by inode.
    pub(super) fn set_dir_mtime(&mut self, ino: u64, mtime: SystemTime) {
        let path = self.get_dir_path(ino);
        self.modified_times.insert(path, mtime);
    }

    /// Gets the mtime for a file by path.
    ///
    /// Checks modified_times first, then falls back to archive.
    pub(super) fn get_file_mtime(&self, path: &str) -> SystemTime {
        // Check if mtime was modified via utimensat.
        if let Some(&mtime) = self.modified_times.get(path) {
            return mtime;
        }

        // Fall back to archive mtime.
        self.archive
            .find_entry_with_path(path)
            .map(|(entry_row, _, _)| {
                let mtime_ms = entry_row.modified_time.get();
                SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(mtime_ms.max(0) as u64)
            })
            .unwrap_or(self.mount_time)
    }

    /// Sets the mtime for a file by path.
    pub(super) fn set_file_mtime(&mut self, path: &str, mtime: SystemTime) {
        self.modified_times.insert(path.to_string(), mtime);
    }

    /// Updates the mtime of a parent directory to the current time.
    ///
    /// Called when entries are created or removed in a directory,
    /// matching POSIX behavior where directory mtime reflects the
    /// last structural change.
    fn touch_dir_mtime(&mut self, parent_ino: u64) {
        self.set_dir_mtime(parent_ino, SystemTime::now());
    }

    /// Syncs all modified files to the archive.
    pub(super) fn sync_modified_to_archive(&mut self) -> Result<(), i32> {
        // Collect paths to avoid borrow issues.
        let data_paths: Vec<String> = self.modified_data.keys().cloned().collect();

        log::trace!(
            "sync_modified_to_archive: {} modified files, {} modified modes, {} modified times",
            data_paths.len(),
            self.modified_modes.len(),
            self.modified_times.len()
        );

        // Sync files with modified data.
        for path in &data_paths {
            let perm = self.get_file_mode(path);
            let archive_mode = SFlag::S_IFREG.bits() | perm.bits();
            let mtime = self.modified_times.get(path).copied();
            let data = self.modified_data.get(path).unwrap().clone();
            // Delete existing entry first to avoid duplicates (create() adds
            // an initial entry, so we must remove it before re-adding with
            // the final content).
            self.archive.delete(path);
            self.archive
                .add_entry_with_mtime(path, &data, archive_mode, mtime)
                .map_err(|_| libc::EIO)?;
        }

        // Sync files with only mode or mtime changes (not data changes).
        let metadata_only_paths: Vec<String> = self
            .modified_modes
            .keys()
            .chain(self.modified_times.keys())
            .filter(|p| !self.modified_data.contains_key(*p))
            .cloned()
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        for path in &metadata_only_paths {
            let perm = self.get_file_mode(path);
            let archive_mode = SFlag::S_IFREG.bits() | perm.bits();
            let mtime = self.modified_times.get(path).copied();
            // Read current data from archive.
            let data = self
                .archive
                .file(path)
                .map_err(|_| libc::EIO)?
                .data()
                .to_vec();
            self.archive.delete(path);
            self.archive
                .add_entry_with_mtime(path, &data, archive_mode, mtime)
                .map_err(|_| libc::EIO)?;
        }

        log::trace!("sync_modified_to_archive: calling archive.sync()");
        self.archive.sync().map_err(|_| libc::EIO)?;
        self.modified_data.clear();
        self.modified_modes.clear();
        self.modified_times.clear();
        Ok(())
    }

    /// Creates a new directory.
    ///
    /// Returns the inode of the new directory and its attributes.
    pub(super) fn create_directory(
        &mut self,
        parent_ino: u64,
        name: &str,
        mode: Mode,
    ) -> Result<(u64, fuser::FileAttr), i32> {
        // Check name length.
        Self::validate_name(name)?;

        // Check parent exists.
        if !self.dir_contents.contains_key(&parent_ino) {
            return Err(libc::ENOENT);
        }

        // Build full path.
        let parent_path = self.get_dir_path(parent_ino);
        let full_path = if parent_path.is_empty() {
            name.to_string()
        } else {
            format!("{}/{}", parent_path, name)
        };

        // Check path length and reserved prefix.
        self.validate_path_length(&full_path)?;
        Self::validate_archive_path(&full_path)?;

        // Check directory doesn't already exist.
        if self.dir_inodes.contains_key(&full_path) {
            return Err(libc::EEXIST);
        }

        // Check no file with that name exists.
        if self.path_to_inode.contains_key(&full_path) {
            return Err(libc::EEXIST);
        }

        // Allocate inode.
        let ino = self.next_dir_ino;
        self.next_dir_ino += 1;

        // Create directory entry.
        self.dir_inodes.insert(full_path.clone(), ino);
        self.dir_contents.insert(ino, Vec::new());

        // Add to parent's contents.
        if let Some(contents) = self.dir_contents.get_mut(&parent_ino) {
            contents.push(FuseDirEntry::directory(name, ino));
        }

        // Add directory entry to archive.
        let archive_mode = SFlag::S_IFDIR.bits() | mode.bits();
        self.archive
            .add_folder(&full_path, archive_mode)
            .map_err(|_| libc::EIO)?;

        // Update parent directory mtime.
        self.touch_dir_mtime(parent_ino);

        let attr = self.get_attr(ino, FileType::Directory);
        Ok((ino, attr))
    }

    /// Removes an empty directory.
    pub(super) fn remove_directory(
        &mut self,
        parent_ino: u64,
        name: &str,
        caller_uid: u32,
    ) -> Result<(), i32> {
        // Check parent exists.
        if !self.dir_contents.contains_key(&parent_ino) {
            return Err(libc::ENOENT);
        }

        // Build full path.
        let parent_path = self.get_dir_path(parent_ino);
        let full_path = if parent_path.is_empty() {
            name.to_string()
        } else {
            format!("{}/{}", parent_path, name)
        };

        // Check directory exists.
        let dir_ino = match self.dir_inodes.get(&full_path) {
            Some(&ino) => ino,
            None => return Err(libc::ENOENT),
        };

        // Check it's not a file.
        if self.path_to_inode.contains_key(&full_path) {
            return Err(libc::ENOTDIR);
        }

        // Check directory is empty.
        if let Some(contents) = self.dir_contents.get(&dir_ino)
            && !contents.is_empty()
        {
            return Err(libc::ENOTEMPTY);
        }

        // Enforce sticky-bit restriction.
        self.check_sticky_bit(parent_ino, &full_path, caller_uid)?;

        // Remove from dir_inodes and dir_contents.
        self.dir_inodes.remove(&full_path);
        self.dir_contents.remove(&dir_ino);

        // Remove from parent's contents.
        if let Some(contents) = self.dir_contents.get_mut(&parent_ino) {
            contents.retain(|e| e.name != name);
        }

        // Remove from archive.
        let _ = self.archive.delete(&full_path);

        // Update parent directory mtime.
        self.touch_dir_mtime(parent_ino);

        Ok(())
    }

    /// Gets the path for a directory inode.
    pub(super) fn get_dir_path(&self, ino: u64) -> String {
        if ino == ROOT_INO {
            return String::new();
        }
        for (path, &dir_ino) in &self.dir_inodes {
            if dir_ino == ino {
                return path.clone();
            }
        }
        String::new()
    }

    /// Gets the parent directory's inode for a given directory inode.
    ///
    /// Returns `ROOT_INO` if the directory is at the root level or not found.
    pub(super) fn get_parent_inode(&self, ino: u64) -> u64 {
        if ino == ROOT_INO {
            return ROOT_INO;
        }

        let path = self.get_dir_path(ino);
        if path.is_empty() {
            return ROOT_INO;
        }

        // Get parent path by removing the last component.
        let parent_path = match path.rfind('/') {
            Some(pos) => &path[..pos],
            None => "", // No slash means parent is root.
        };

        // Look up parent inode.
        self.dir_inodes
            .get(parent_path)
            .copied()
            .unwrap_or(ROOT_INO)
    }

    /// Returns the depth of a directory (number of path components).
    ///
    /// Root returns 0, `"a"` returns 1, `"a/b"` returns 2, etc.
    pub(super) fn dir_depth(&self, ino: u64) -> usize {
        if ino == ROOT_INO {
            return 0;
        }
        let path = self.get_dir_path(ino);
        if path.is_empty() {
            return 0;
        }
        path.chars().filter(|&c| c == '/').count() + 1
    }

    /// Creates a new file.
    ///
    /// Returns the inode of the new file and its attributes.
    pub(super) fn create_file(
        &mut self,
        parent_ino: u64,
        name: &str,
        mode: Mode,
    ) -> Result<(u64, fuser::FileAttr), i32> {
        // Check name length.
        Self::validate_name(name)?;

        // Check parent exists.
        if !self.dir_contents.contains_key(&parent_ino) {
            return Err(libc::ENOENT);
        }

        // Build full path.
        let parent_path = self.get_dir_path(parent_ino);
        let full_path = if parent_path.is_empty() {
            name.to_string()
        } else {
            format!("{}/{}", parent_path, name)
        };

        // Check path length and reserved prefix.
        self.validate_path_length(&full_path)?;
        Self::validate_archive_path(&full_path)?;

        // Check file doesn't already exist.
        if self.path_to_inode.contains_key(&full_path) {
            return Err(libc::EEXIST);
        }

        // Check no directory with that name exists.
        if self.dir_inodes.contains_key(&full_path) {
            return Err(libc::EEXIST);
        }

        // Allocate inode.
        let ino = self.next_file_ino;
        self.next_file_ino += 1;

        // Add to path mappings.
        self.inode_to_path.insert(ino, full_path.clone());
        self.path_to_inode.insert(full_path.clone(), ino);

        // Add to parent's contents.
        if let Some(contents) = self.dir_contents.get_mut(&parent_ino) {
            contents.push(FuseDirEntry::file(name, ino));
        }

        // Initialize empty file in modified_data.
        self.modified_data.insert(full_path.clone(), Vec::new());

        // Add empty file to archive.
        let archive_mode = SFlag::S_IFREG.bits() | mode.bits();
        self.archive
            .add_entry(&full_path, &[], archive_mode)
            .map_err(|_| libc::EIO)?;

        // Update parent directory mtime.
        self.touch_dir_mtime(parent_ino);

        let perm = mode.bits() as u16;
        let attr = fuser::FileAttr {
            ino,
            size: 0,
            blocks: 0,
            atime: self.mount_time,
            mtime: self.mount_time,
            ctime: self.mount_time,
            crtime: self.mount_time,
            kind: FileType::RegularFile,
            perm: if perm == 0 {
                DEFAULT_FILE_PERM.bits() as u16
            } else {
                perm
            },
            nlink: 1,
            uid: self.get_uid(&full_path),
            gid: self.get_gid(&full_path),
            rdev: 0,
            blksize: 4096,
            flags: 0,
        };

        Ok((ino, attr))
    }

    /// Checks whether a caller is allowed to modify an entry in a directory
    /// that has the sticky bit set.
    ///
    /// When a directory has the sticky bit (01000), only root (uid 0), the
    /// directory owner, or the entry owner may remove/rename entries.
    ///
    /// Returns `Ok(())` if permitted, `Err(EACCES)` if denied.
    fn check_sticky_bit(
        &self,
        parent_ino: u64,
        entry_path: &str,
        caller_uid: u32,
    ) -> Result<(), i32> {
        let parent_mode = self.get_dir_mode(parent_ino);
        if !parent_mode.contains(Mode::S_ISVTX) {
            return Ok(());
        }

        // Root bypasses sticky-bit checks.
        if caller_uid == 0 {
            return Ok(());
        }

        // Check if caller owns the parent directory.
        let parent_path = self.get_dir_path(parent_ino);
        let dir_uid = self
            .modified_uids
            .get(&parent_path)
            .copied()
            .unwrap_or(self.uid);
        if caller_uid == dir_uid {
            return Ok(());
        }

        // Check if caller owns the entry.
        let entry_uid = self.get_uid(entry_path);
        if caller_uid == entry_uid {
            return Ok(());
        }

        Err(libc::EACCES)
    }

    /// Removes a file.
    pub(super) fn remove_file(
        &mut self,
        parent_ino: u64,
        name: &str,
        caller_uid: u32,
    ) -> Result<(), i32> {
        // Check parent exists.
        if !self.dir_contents.contains_key(&parent_ino) {
            return Err(libc::ENOENT);
        }

        // Build full path.
        let parent_path = self.get_dir_path(parent_ino);
        let full_path = if parent_path.is_empty() {
            name.to_string()
        } else {
            format!("{}/{}", parent_path, name)
        };

        // Check file exists.
        let file_ino = match self.path_to_inode.get(&full_path) {
            Some(&ino) => ino,
            None => return Err(libc::ENOENT),
        };

        // Check it's not a directory.
        if self.dir_inodes.contains_key(&full_path) {
            return Err(libc::EISDIR);
        }

        // Enforce sticky-bit restriction.
        self.check_sticky_bit(parent_ino, &full_path, caller_uid)?;

        // Remove this path from mappings.
        self.path_to_inode.remove(&full_path);

        // Remove from parent's contents.
        if let Some(contents) = self.dir_contents.get_mut(&parent_ino) {
            contents.retain(|e| e.name != name);
        }

        // Decrement nlink and handle hard-link-aware cleanup.
        let nlink = self.nlink_counts.get(&file_ino).copied().unwrap_or(1);
        if nlink > 1 {
            // Other links remain — only remove this path.
            self.nlink_counts.insert(file_ino, nlink - 1);

            // If inode_to_path pointed to the removed path, update it
            // to point to another remaining path for this inode.
            if self.inode_to_path.get(&file_ino).map(String::as_str) == Some(&full_path) {
                let alt_path = self
                    .path_to_inode
                    .iter()
                    .find(|&(_, &ino)| ino == file_ino)
                    .map(|(p, _)| p.clone());
                if let Some(alt) = alt_path {
                    self.inode_to_path.insert(file_ino, alt);
                } else {
                    self.inode_to_path.remove(&file_ino);
                }
            }

            // Use unlink (removes only the directory row; preserves
            // the entry row while other links reference it).
            let _ = self.archive.unlink(&full_path);
        } else {
            // Last link — full cleanup.
            self.inode_to_path.remove(&file_ino);
            self.nlink_counts.remove(&file_ino);
            self.modified_data.remove(&full_path);
            let _ = self.archive.unlink(&full_path);
        }

        // Update parent directory mtime.
        self.touch_dir_mtime(parent_ino);

        Ok(())
    }

    /// Renames/moves a file or directory.
    pub(super) fn rename_entry(
        &mut self,
        old_parent_ino: u64,
        old_name: &str,
        new_parent_ino: u64,
        new_name: &str,
        caller_uid: u32,
    ) -> Result<(), i32> {
        // Check new name length.
        Self::validate_name(new_name)?;

        // Check both parents exist.
        if !self.dir_contents.contains_key(&old_parent_ino) {
            return Err(libc::ENOENT);
        }
        if !self.dir_contents.contains_key(&new_parent_ino) {
            return Err(libc::ENOENT);
        }

        // Build paths.
        let old_parent_path = self.get_dir_path(old_parent_ino);
        let new_parent_path = self.get_dir_path(new_parent_ino);

        let old_path = if old_parent_path.is_empty() {
            old_name.to_string()
        } else {
            format!("{}/{}", old_parent_path, old_name)
        };

        let new_path = if new_parent_path.is_empty() {
            new_name.to_string()
        } else {
            format!("{}/{}", new_parent_path, new_name)
        };

        // Check new path length and reserved prefix.
        self.validate_path_length(&new_path)?;
        Self::validate_archive_path(&new_path)?;

        // Check if source is a file or directory.
        let is_file = self.path_to_inode.contains_key(&old_path);
        let is_dir = self.dir_inodes.contains_key(&old_path);

        if !is_file && !is_dir {
            return Err(libc::ENOENT);
        }

        // Enforce sticky-bit restriction on source directory.
        self.check_sticky_bit(old_parent_ino, &old_path, caller_uid)?;

        // Check destination doesn't exist (or handle overwrite for files).
        if self.dir_inodes.contains_key(&new_path) {
            if is_file {
                return Err(libc::EISDIR);
            }
            return Err(libc::EEXIST);
        }
        if self.path_to_inode.contains_key(&new_path) {
            // Destination file exists - remove it first.
            if is_dir {
                // Can't overwrite file with directory.
                return Err(libc::ENOTDIR);
            }
            // Remove destination file and all associated metadata.
            let dest_ino = *self.path_to_inode.get(&new_path).unwrap();
            self.inode_to_path.remove(&dest_ino);
            self.path_to_inode.remove(&new_path);
            self.modified_data.remove(&new_path);
            self.modified_modes.remove(&new_path);
            self.modified_times.remove(&new_path);
            self.modified_uids.remove(&new_path);
            self.modified_gids.remove(&new_path);
            self.nlink_counts.remove(&dest_ino);
            let _ = self.archive.delete(&new_path);
            if let Some(contents) = self.dir_contents.get_mut(&new_parent_ino) {
                contents.retain(|e| e.name != new_name);
            }
        }

        if is_file {
            // Rename file.
            let ino = *self.path_to_inode.get(&old_path).unwrap();

            // Update path mappings.
            self.path_to_inode.remove(&old_path);
            self.path_to_inode.insert(new_path.clone(), ino);
            self.inode_to_path.insert(ino, new_path.clone());

            // Move modified data if present.
            if let Some(data) = self.modified_data.remove(&old_path) {
                self.modified_data.insert(new_path.clone(), data);
            }

            // Move transient ownership overrides if present.
            if let Some(uid) = self.modified_uids.remove(&old_path) {
                self.modified_uids.insert(new_path.clone(), uid);
            }
            if let Some(gid) = self.modified_gids.remove(&old_path) {
                self.modified_gids.insert(new_path.clone(), gid);
            }
            if let Some(mode) = self.modified_modes.remove(&old_path) {
                self.modified_modes.insert(new_path.clone(), mode);
            }
            if let Some(time) = self.modified_times.remove(&old_path) {
                self.modified_times.insert(new_path.clone(), time);
            }

            // Determine original file type (regular file vs symlink).
            let is_symlink = self
                .dir_contents
                .get(&old_parent_ino)
                .and_then(|c| c.iter().find(|e| e.name == old_name))
                .is_some_and(|e| e.kind == FileType::Symlink);

            // Update parent directory contents.
            if let Some(contents) = self.dir_contents.get_mut(&old_parent_ino) {
                contents.retain(|e| e.name != old_name);
            }
            if let Some(contents) = self.dir_contents.get_mut(&new_parent_ino) {
                let entry = if is_symlink {
                    FuseDirEntry::symlink(new_name, ino)
                } else {
                    FuseDirEntry::file(new_name, ino)
                };
                contents.push(entry);
            }

            // Update archive: copy data from old path to new, delete old.
            if is_symlink {
                let target = self
                    .archive
                    .symlink(&old_path)
                    .ok()
                    .and_then(|link| link.target().map(String::from));
                if let Some(target) = target {
                    let _ = self.archive.add_symlink(&new_path, &target, 0o777);
                }
            } else if let Ok(file) = self.archive.file(&old_path) {
                let data = file.data().to_vec();
                let perm = self.get_file_mode(&old_path);
                let archive_mode = SFlag::S_IFREG.bits() | perm.bits();
                let _ = self.archive.add_entry(&new_path, &data, archive_mode);
            } else if let Some(data) = self.modified_data.get(&new_path) {
                let _ = self.archive.add_entry(&new_path, data, DEFAULT_FILE_MODE);
            }
            let _ = self.archive.delete(&old_path);
        } else {
            // Rename directory.
            let ino = *self.dir_inodes.get(&old_path).unwrap();

            // Update directory mappings.
            self.dir_inodes.remove(&old_path);
            self.dir_inodes.insert(new_path.clone(), ino);

            // Update parent directory contents.
            if let Some(contents) = self.dir_contents.get_mut(&old_parent_ino) {
                contents.retain(|e| e.name != old_name);
            }
            if let Some(contents) = self.dir_contents.get_mut(&new_parent_ino) {
                contents.push(FuseDirEntry::directory(new_name, ino));
            }

            // Update all child paths (files and subdirectories).
            let old_prefix = format!("{}/", old_path);
            let new_prefix = format!("{}/", new_path);

            // Update file paths.
            let file_updates: Vec<(u64, String, String)> = self
                .inode_to_path
                .iter()
                .filter(|(_, p)| p.starts_with(&old_prefix))
                .map(|(&ino, p)| {
                    let new_p = format!("{}{}", new_prefix, &p[old_prefix.len()..]);
                    (ino, p.clone(), new_p)
                })
                .collect();

            for (ino, old_p, new_p) in file_updates {
                self.path_to_inode.remove(&old_p);
                self.path_to_inode.insert(new_p.clone(), ino);
                self.inode_to_path.insert(ino, new_p.clone());
                if let Some(data) = self.modified_data.remove(&old_p) {
                    self.modified_data.insert(new_p.clone(), data);
                }
                if let Some(uid) = self.modified_uids.remove(&old_p) {
                    self.modified_uids.insert(new_p.clone(), uid);
                }
                if let Some(gid) = self.modified_gids.remove(&old_p) {
                    self.modified_gids.insert(new_p.clone(), gid);
                }
                if let Some(mode) = self.modified_modes.remove(&old_p) {
                    self.modified_modes.insert(new_p.clone(), mode);
                }
                if let Some(time) = self.modified_times.remove(&old_p) {
                    self.modified_times.insert(new_p, time);
                }
            }

            // Update subdirectory paths.
            let dir_updates: Vec<(u64, String, String)> = self
                .dir_inodes
                .iter()
                .filter(|(p, _)| p.starts_with(&old_prefix))
                .map(|(p, &ino)| {
                    let new_p = format!("{}{}", new_prefix, &p[old_prefix.len()..]);
                    (ino, p.clone(), new_p)
                })
                .collect();

            for (ino, old_p, new_p) in dir_updates {
                self.dir_inodes.remove(&old_p);
                self.dir_inodes.insert(new_p.clone(), ino);
                if let Some(uid) = self.modified_uids.remove(&old_p) {
                    self.modified_uids.insert(new_p.clone(), uid);
                }
                if let Some(gid) = self.modified_gids.remove(&old_p) {
                    self.modified_gids.insert(new_p.clone(), gid);
                }
                if let Some(mode) = self.modified_modes.remove(&old_p) {
                    self.modified_modes.insert(new_p.clone(), mode);
                }
                if let Some(time) = self.modified_times.remove(&old_p) {
                    self.modified_times.insert(new_p, time);
                }
            }

            // Propagate the renamed directory's own metadata overrides.
            if let Some(uid) = self.modified_uids.remove(&old_path) {
                self.modified_uids.insert(new_path.clone(), uid);
            }
            if let Some(gid) = self.modified_gids.remove(&old_path) {
                self.modified_gids.insert(new_path.clone(), gid);
            }
            if let Some(mode) = self.modified_modes.remove(&old_path) {
                self.modified_modes.insert(new_path.clone(), mode);
            }
            if let Some(time) = self.modified_times.remove(&old_path) {
                self.modified_times.insert(new_path.clone(), time);
            }

            // Update archive: add new dir entry, remove old.
            let _ = self.archive.add_folder(&new_path, DEFAULT_DIR_MODE);
            let _ = self.archive.delete(&old_path);
        }

        // Update parent directory mtimes.
        self.touch_dir_mtime(old_parent_ino);
        if new_parent_ino != old_parent_ino {
            self.touch_dir_mtime(new_parent_ino);
        }

        Ok(())
    }

    /// Lexically normalizes a path without filesystem access.
    ///
    /// Resolves `.` (current dir), `..` (parent dir), and redundant
    /// separators. Unlike `canonicalize`, this does not require the path
    /// to exist on disk.
    fn normalize_path(path: &Path) -> PathBuf {
        let mut parts: Vec<Component<'_>> = Vec::new();
        for c in path.components() {
            match c {
                Component::ParentDir => {
                    if matches!(parts.last(), Some(Component::Normal(_))) {
                        parts.pop();
                    }
                }
                Component::CurDir => {}
                _ => parts.push(c),
            }
        }
        parts.iter().collect()
    }

    /// Creates a new symlink.
    ///
    /// Absolute symlink targets are normalized to archive-relative paths
    /// by stripping the mount prefix (when known). The writer rejects
    /// absolute targets, so this normalization is required for tooling
    /// that creates absolute symlinks pointing inside the mount.
    ///
    /// Returns the inode of the new symlink and its attributes.
    pub(super) fn create_symlink(
        &mut self,
        parent_ino: u64,
        name: &str,
        target: &str,
    ) -> Result<(u64, fuser::FileAttr), i32> {
        // Check name length.
        Self::validate_name(name)?;

        // Check parent exists.
        if !self.dir_contents.contains_key(&parent_ino) {
            return Err(libc::ENOENT);
        }

        // Build full path.
        let parent_path = self.get_dir_path(parent_ino);
        let full_path = if parent_path.is_empty() {
            name.to_string()
        } else {
            format!("{}/{}", parent_path, name)
        };

        // Check path length and reserved prefix.
        self.validate_path_length(&full_path)?;
        Self::validate_archive_path(&full_path)?;

        // Check symlink doesn't already exist.
        if self.path_to_inode.contains_key(&full_path) {
            return Err(libc::EEXIST);
        }

        // Check no directory with that name exists.
        if self.dir_inodes.contains_key(&full_path) {
            return Err(libc::EEXIST);
        }

        // Convert absolute symlink targets to archive-relative paths.
        // Relative targets pass through unchanged — the writer validates them.
        let target = {
            let path = Path::new(target);
            if path.has_root() {
                let normalized = Self::normalize_path(path);
                let mount = self.mount_point.as_deref().ok_or(libc::EINVAL)?;
                let stripped = normalized.strip_prefix(mount).map_err(|_| libc::EINVAL)?;
                let s = stripped.to_str().ok_or(libc::EINVAL)?;
                if s.is_empty() {
                    return Err(libc::EINVAL);
                }
                s.to_string()
            } else {
                target.to_string()
            }
        };

        // Allocate inode.
        let ino = self.next_file_ino;
        self.next_file_ino += 1;

        // Add to path mappings.
        self.inode_to_path.insert(ino, full_path.clone());
        self.path_to_inode.insert(full_path.clone(), ino);

        // Add to parent's contents.
        if let Some(contents) = self.dir_contents.get_mut(&parent_ino) {
            contents.push(FuseDirEntry::symlink(name, ino));
        }

        // Add symlink to archive.
        self.archive
            .add_symlink(&full_path, &target, 0o777)
            .map_err(|e| match e {
                BaleError::UnsafeFilename(_) | BaleError::PathTooLong { .. } => libc::ENAMETOOLONG,
                BaleError::PathExists(_) => libc::EEXIST,
                _ => libc::EIO,
            })?;

        // Update parent directory mtime.
        self.touch_dir_mtime(parent_ino);

        let attr = fuser::FileAttr {
            ino,
            size: target.len() as u64,
            blocks: 0,
            atime: self.mount_time,
            mtime: self.mount_time,
            ctime: self.mount_time,
            crtime: self.mount_time,
            kind: FileType::Symlink,
            perm: 0o777,
            nlink: 1,
            uid: self.get_uid(&full_path),
            gid: self.get_gid(&full_path),
            rdev: 0,
            blksize: 4096,
            flags: 0,
        };

        Ok((ino, attr))
    }

    /// Creates a hard link to an existing file.
    ///
    /// Returns the shared inode and updated attributes for the linked file.
    pub(super) fn create_hard_link(
        &mut self,
        ino: u64,
        new_parent_ino: u64,
        new_name: &str,
    ) -> Result<(u64, fuser::FileAttr), i32> {
        // Check name validity.
        Self::validate_name(new_name)?;

        // Check the source inode exists and is a file (not a directory).
        let source_path = match self.inode_to_path.get(&ino) {
            Some(p) => p.clone(),
            None => return Err(libc::ENOENT),
        };

        // Check parent exists.
        if !self.dir_contents.contains_key(&new_parent_ino) {
            return Err(libc::ENOENT);
        }

        // Build full path for the new link.
        let parent_path = self.get_dir_path(new_parent_ino);
        let full_path = if parent_path.is_empty() {
            new_name.to_string()
        } else {
            format!("{}/{}", parent_path, new_name)
        };

        // Check path length and reserved prefix.
        self.validate_path_length(&full_path)?;
        Self::validate_archive_path(&full_path)?;

        // Check destination doesn't already exist.
        if self.path_to_inode.contains_key(&full_path) {
            return Err(libc::EEXIST);
        }
        if self.dir_inodes.contains_key(&full_path) {
            return Err(libc::EEXIST);
        }

        // Create hard link in archive.
        self.archive
            .hard_link(&source_path, &full_path)
            .map_err(|_| libc::EIO)?;

        // Update path mappings — share the same inode.
        self.path_to_inode.insert(full_path.clone(), ino);

        // Update nlink count.
        *self.nlink_counts.entry(ino).or_insert(1) += 1;
        let nlink = self.nlink_counts.get(&ino).copied().unwrap_or(1);

        // Add to parent directory contents.
        if let Some(contents) = self.dir_contents.get_mut(&new_parent_ino) {
            contents.push(FuseDirEntry::file(new_name, ino));
        }

        // Update parent directory mtime.
        self.touch_dir_mtime(new_parent_ino);

        // Build attributes from archive entry.
        let attr = if let Some((entry_row, _, _)) = self.archive.find_entry_with_path(&full_path) {
            let mut attr =
                self.get_attr_for_file(ino, FileType::RegularFile, entry_row, &full_path);
            attr.nlink = nlink;
            attr
        } else {
            self.get_attr(ino, FileType::RegularFile)
        };

        Ok((ino, attr))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Creates a test archive with the given entries.
    fn create_test_archive(entries: &[(&str, &[u8], u32)]) -> ArchiveWriter {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.bale");

        let mut writer = ArchiveWriter::create(&path).unwrap();
        for (name, data, mode) in entries {
            writer.add_entry(name, data, *mode).unwrap();
        }
        writer.sync().unwrap();
        drop(writer);

        // Re-open to get a fresh state.
        let writer = ArchiveWriter::open(&path).unwrap();

        // Keep temp directory alive by leaking it.
        std::mem::forget(dir);

        writer
    }

    /// Creates a test archive with hard links.
    fn create_test_archive_with_links(
        entries: &[(&str, &[u8], u32)],
        links: &[(&str, &str)],
    ) -> ArchiveWriter {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.bale");

        let mut writer = ArchiveWriter::create(&path).unwrap();
        for (name, data, mode) in entries {
            writer.add_entry(name, data, *mode).unwrap();
        }
        for (target, link) in links {
            writer.hard_link(target, link).unwrap();
        }
        writer.sync().unwrap();
        drop(writer);

        let writer = ArchiveWriter::open(&path).unwrap();
        std::mem::forget(dir);
        writer
    }

    /// Tests that hard-linked paths share the same inode.
    #[test]
    fn hard_links_share_inodes() {
        let archive = create_test_archive_with_links(
            &[("original.txt", b"shared data", 0o100644)],
            &[("original.txt", "link.txt")],
        );

        let state = BaleFsState::new(archive, false, 1000, 1000);

        // Both paths should map to the same inode.
        let original_ino = *state.path_to_inode.get("original.txt").unwrap();
        let link_ino = *state.path_to_inode.get("link.txt").unwrap();
        assert_eq!(
            original_ino, link_ino,
            "hard-linked paths should share an inode"
        );

        // nlink count should be 2.
        let nlink = state.nlink_counts.get(&original_ino).copied().unwrap_or(1);
        assert_eq!(nlink, 2, "hard-linked file should have nlink=2");
    }

    /// Tests creating a hard link via `create_hard_link`.
    #[test]
    fn create_hard_link_via_fuse() {
        let archive = create_test_archive(&[("file.txt", b"hello", 0o100644)]);
        let mut state = BaleFsState::new(archive, false, 1000, 1000);

        // Get the inode for the existing file.
        let file_ino = *state.path_to_inode.get("file.txt").unwrap();

        // Create a hard link in the root directory.
        let result = state.create_hard_link(file_ino, ROOT_INO, "link.txt");
        assert!(result.is_ok(), "create_hard_link should succeed");

        let (link_ino, attr) = result.unwrap();
        assert_eq!(link_ino, file_ino, "hard link should share the same inode");
        assert_eq!(attr.nlink, 2, "nlink should be 2 after creating hard link");

        // Verify the new path is registered.
        assert_eq!(
            state.path_to_inode.get("link.txt").copied(),
            Some(file_ino),
            "new path should map to the same inode"
        );

        // Verify the new link appears in the root directory listing.
        let root_contents = state.dir_contents.get(&ROOT_INO).unwrap();
        assert!(
            root_contents.iter().any(|e| e.name == "link.txt"),
            "link.txt should appear in root directory"
        );
    }

    /// Tests that creating a hard link to an existing path fails.
    #[test]
    fn create_hard_link_existing_path_fails() {
        let archive = create_test_archive(&[
            ("file.txt", b"hello", 0o100644),
            ("other.txt", b"world", 0o100644),
        ]);
        let mut state = BaleFsState::new(archive, false, 1000, 1000);

        let file_ino = *state.path_to_inode.get("file.txt").unwrap();
        let result = state.create_hard_link(file_ino, ROOT_INO, "other.txt");
        assert_eq!(result.unwrap_err(), libc::EEXIST);
    }

    /// Tests that `get_parent_inode` returns the correct parent for nested directories.
    #[test]
    fn parent_inode_lookup() {
        // Create archive with nested directories.
        let archive = create_test_archive(&[
            ("a/", &[], 0o40755),
            ("a/b/", &[], 0o40755),
            ("a/b/c/", &[], 0o40755),
            ("a/b/file.txt", b"test", 0o100644),
        ]);

        let state = BaleFsState::new(archive, false, 1000, 1000);

        // Get directory inodes.
        let root_ino = ROOT_INO;
        let a_ino = *state.dir_inodes.get("a").unwrap();
        let ab_ino = *state.dir_inodes.get("a/b").unwrap();
        let abc_ino = *state.dir_inodes.get("a/b/c").unwrap();

        // Verify parent lookups.
        assert_eq!(
            state.get_parent_inode(root_ino),
            ROOT_INO,
            "root's parent should be root"
        );
        assert_eq!(
            state.get_parent_inode(a_ino),
            ROOT_INO,
            "a's parent should be root"
        );
        assert_eq!(
            state.get_parent_inode(ab_ino),
            a_ino,
            "a/b's parent should be a"
        );
        assert_eq!(
            state.get_parent_inode(abc_ino),
            ab_ino,
            "a/b/c's parent should be a/b"
        );
    }

    /// Tests that `mkdir` respects the mode parameter.
    #[test]
    fn mkdir_respects_mode() {
        let archive = create_test_archive(&[]);
        let mut state = BaleFsState::new(archive, false, 1000, 1000);

        // Create directory with mode 0o700 (rwx------).
        let result = state.create_directory(ROOT_INO, "private", Mode::S_IRWXU);
        assert!(result.is_ok());

        // Sync to persist the entry.
        state.archive.sync().unwrap();

        // Find the entry in the archive and verify mode.
        let entries: Vec<_> = state.archive.iter_entries().collect();
        let (entry_row, _path) = entries
            .iter()
            .find(|(_, p)| p.starts_with(b"private"))
            .unwrap();

        // Mode is stored directly in the entry row.
        let mode = entry_row.mode.get();

        // Mode should be 0o40700 (directory bit + rwx------).
        assert_eq!(mode, 0o40700, "directory mode should be 0o40700");
    }

    /// Tests that `modified_data` is cleared after sync.
    #[test]
    fn modified_data_cleared_after_sync() {
        let archive = create_test_archive(&[("test.txt", b"original", 0o100644)]);
        let mut state = BaleFsState::new(archive, false, 1000, 1000);

        // Modify the file.
        state
            .modified_data
            .insert("test.txt".to_string(), b"modified".to_vec());
        assert!(!state.modified_data.is_empty());

        // Sync to archive.
        state.sync_modified_to_archive().unwrap();

        // modified_data should be cleared.
        assert!(
            state.modified_data.is_empty(),
            "modified_data should be cleared after sync"
        );
    }

    // ==================== normalize_path Tests ====================

    /// Tests that `.` and `..` components are resolved.
    #[test]
    fn normalize_path_resolves_dot_and_dotdot() {
        let result = BaleFsState::normalize_path(Path::new("/a/b/../c/./d"));
        assert_eq!(result, PathBuf::from("/a/c/d"));
    }

    /// Tests that multiple slashes are collapsed.
    #[test]
    fn normalize_path_collapses_slashes() {
        let result = BaleFsState::normalize_path(Path::new("/a///b"));
        assert_eq!(result, PathBuf::from("/a/b"));
    }

    /// Tests that relative paths pass through correctly.
    #[test]
    fn normalize_path_relative_passthrough() {
        let result = BaleFsState::normalize_path(Path::new("a/b/c"));
        assert_eq!(result, PathBuf::from("a/b/c"));
    }

    // ==================== create_symlink absolute target Tests ====================

    /// Tests that an absolute target inside the mount is normalized to relative.
    #[test]
    fn create_symlink_normalizes_absolute_target() {
        let archive = create_test_archive(&[]);
        let mut state = BaleFsState::new(archive, false, 1000, 1000);
        state.set_mount_point(PathBuf::from("/mnt/archive"));

        let result = state.create_symlink(ROOT_INO, "link", "/mnt/archive/foo/bar.txt");
        assert!(
            result.is_ok(),
            "should normalize absolute target inside mount"
        );

        // Verify stored target is relative.
        let symlink = state.archive.symlink("link").unwrap();
        assert_eq!(symlink.target(), Some("foo/bar.txt"));
    }

    /// Tests that an absolute target with `..` is normalized correctly.
    #[test]
    fn create_symlink_normalizes_absolute_with_dotdot() {
        let archive = create_test_archive(&[]);
        let mut state = BaleFsState::new(archive, false, 1000, 1000);
        state.set_mount_point(PathBuf::from("/mnt/archive"));

        let result = state.create_symlink(ROOT_INO, "link", "/mnt/archive/a/../b");
        assert!(result.is_ok(), "should normalize absolute target with ..");

        let symlink = state.archive.symlink("link").unwrap();
        assert_eq!(symlink.target(), Some("b"));
    }

    /// Tests that an absolute target outside the mount is rejected.
    #[test]
    fn create_symlink_rejects_absolute_outside_mount() {
        let archive = create_test_archive(&[]);
        let mut state = BaleFsState::new(archive, false, 1000, 1000);
        state.set_mount_point(PathBuf::from("/mnt/archive"));

        let result = state.create_symlink(ROOT_INO, "link", "/usr/share/data.txt");
        assert_eq!(result.unwrap_err(), libc::EINVAL);
    }

    /// Tests that an absolute target pointing to the mount root is rejected.
    #[test]
    fn create_symlink_rejects_mount_root_target() {
        let archive = create_test_archive(&[]);
        let mut state = BaleFsState::new(archive, false, 1000, 1000);
        state.set_mount_point(PathBuf::from("/mnt/archive"));

        let result = state.create_symlink(ROOT_INO, "link", "/mnt/archive");
        assert_eq!(result.unwrap_err(), libc::EINVAL);
    }

    /// Tests that an absolute target without mount point set is rejected.
    #[test]
    fn create_symlink_rejects_absolute_without_mount_point() {
        let archive = create_test_archive(&[]);
        let mut state = BaleFsState::new(archive, false, 1000, 1000);
        // mount_point is None by default.

        let result = state.create_symlink(ROOT_INO, "link", "/foo");
        assert_eq!(result.unwrap_err(), libc::EINVAL);
    }

    /// Tests that relative symlink targets still work unmodified.
    #[test]
    fn create_symlink_relative_target_unchanged() {
        let archive = create_test_archive(&[]);
        let mut state = BaleFsState::new(archive, false, 1000, 1000);
        state.set_mount_point(PathBuf::from("/mnt/archive"));

        let result = state.create_symlink(ROOT_INO, "link", "foo/bar.txt");
        assert!(result.is_ok(), "relative target should work as before");

        let symlink = state.archive.symlink("link").unwrap();
        assert_eq!(symlink.target(), Some("foo/bar.txt"));
    }

    // ==================== Sticky / setuid / setgid bit tests ====================

    /// Tests that chmod preserves the sticky bit (01755).
    #[test]
    fn chmod_preserves_sticky_bit() {
        let archive = create_test_archive(&[("file.txt", b"data", 0o100644)]);
        let mut state = BaleFsState::new(archive, false, 1000, 1000);

        // Simulate chmod to 01755 (sticky + rwxr-xr-x).
        state
            .modified_modes
            .insert("file.txt".to_string(), Mode::from_bits_truncate(0o1755));

        let mode = state.get_file_mode("file.txt");
        // The sticky bit (0o1000) should be preserved.
        assert_eq!(
            mode.bits(),
            0o1755,
            "sticky bit should be preserved in mode"
        );
    }

    /// Tests that chmod preserves setuid and setgid bits.
    #[test]
    fn chmod_preserves_setuid_setgid() {
        let archive = create_test_archive(&[("file.txt", b"data", 0o100644)]);
        let mut state = BaleFsState::new(archive, false, 1000, 1000);

        // Simulate chmod to 04755 (setuid).
        state
            .modified_modes
            .insert("file.txt".to_string(), Mode::from_bits_truncate(0o4755));
        let mode = state.get_file_mode("file.txt");
        assert_eq!(
            mode.bits(),
            0o4755,
            "setuid bit should be preserved in mode"
        );

        // Simulate chmod to 02755 (setgid).
        state
            .modified_modes
            .insert("file.txt".to_string(), Mode::from_bits_truncate(0o2755));
        let mode = state.get_file_mode("file.txt");
        assert_eq!(
            mode.bits(),
            0o2755,
            "setgid bit should be preserved in mode"
        );
    }

    /// Tests that `get_attr_for_file` reports the sticky bit from archive entry.
    #[test]
    fn get_attr_reports_sticky_bit() {
        // S_IFREG | sticky | 0o755.
        let archive = create_test_archive(&[("file.txt", b"data", 0o101755)]);
        let state = BaleFsState::new(archive, false, 1000, 1000);

        let ino = *state.path_to_inode.get("file.txt").unwrap();
        let (entry_row, _, _) = state.archive.find_entry_with_path("file.txt").unwrap();
        let attr = state.get_attr_for_file(ino, FileType::RegularFile, entry_row, "file.txt");

        // perm should include sticky bit: 01755 not just 0755.
        assert_eq!(attr.perm, 0o1755, "perm should include sticky bit");
    }

    // ==================== Rename type checking tests ====================

    /// Tests that renaming a file over an existing directory returns EISDIR.
    #[test]
    fn rename_file_over_directory_returns_eisdir() {
        let archive =
            create_test_archive(&[("src.txt", b"data", 0o100644), ("dest/", &[], 0o40755)]);
        let mut state = BaleFsState::new(archive, false, 1000, 1000);

        let result = state.rename_entry(ROOT_INO, "src.txt", ROOT_INO, "dest", 0);
        assert_eq!(
            result.unwrap_err(),
            libc::EISDIR,
            "renaming file over directory should return EISDIR"
        );
    }

    // ==================== Sticky-bit rename enforcement tests ====================

    /// Tests that rename in a sticky directory by non-owner fails with EACCES.
    #[test]
    fn rename_in_sticky_dir_by_non_owner_fails() {
        let archive = create_test_archive(&[
            ("sticky/", &[], 0o41777),
            ("sticky/file.txt", b"data", 0o100644),
        ]);
        let mut state = BaleFsState::new(archive, false, 1000, 1000);

        // Directory owned by uid 1000, file owned by uid 1000.
        let sticky_ino = *state.dir_inodes.get("sticky").unwrap();

        // Rename as uid 2000 (not root, not file owner, not dir owner).
        let result = state.rename_entry(sticky_ino, "file.txt", sticky_ino, "renamed.txt", 2000);
        assert_eq!(
            result.unwrap_err(),
            libc::EACCES,
            "non-owner rename in sticky dir should fail"
        );
    }

    /// Tests that rename in a sticky directory by file owner succeeds.
    #[test]
    fn rename_in_sticky_dir_by_file_owner_succeeds() {
        let archive = create_test_archive(&[
            ("sticky/", &[], 0o41777),
            ("sticky/file.txt", b"data", 0o100644),
        ]);
        let mut state = BaleFsState::new(archive, false, 1000, 1000);

        let sticky_ino = *state.dir_inodes.get("sticky").unwrap();

        // Rename as uid 1000 (file owner = mount uid).
        let result = state.rename_entry(sticky_ino, "file.txt", sticky_ino, "renamed.txt", 1000);
        assert!(
            result.is_ok(),
            "file owner should be able to rename in sticky dir"
        );
    }

    /// Tests that rename in a sticky directory by root succeeds.
    #[test]
    fn rename_in_sticky_dir_by_root_succeeds() {
        let archive = create_test_archive(&[
            ("sticky/", &[], 0o41777),
            ("sticky/file.txt", b"data", 0o100644),
        ]);
        let mut state = BaleFsState::new(archive, false, 1000, 1000);

        let sticky_ino = *state.dir_inodes.get("sticky").unwrap();

        // Rename as root (uid 0).
        let result = state.rename_entry(sticky_ino, "file.txt", sticky_ino, "renamed.txt", 0);
        assert!(
            result.is_ok(),
            "root should be able to rename in sticky dir"
        );
    }

    /// Tests that unlink in a sticky directory by non-owner fails with EACCES.
    #[test]
    fn unlink_in_sticky_dir_by_non_owner_fails() {
        let archive = create_test_archive(&[
            ("sticky/", &[], 0o41777),
            ("sticky/file.txt", b"data", 0o100644),
        ]);
        let mut state = BaleFsState::new(archive, false, 1000, 1000);

        let sticky_ino = *state.dir_inodes.get("sticky").unwrap();

        // Unlink as uid 2000 (not root, not file owner, not dir owner).
        let result = state.remove_file(sticky_ino, "file.txt", 2000);
        assert_eq!(
            result.unwrap_err(),
            libc::EACCES,
            "non-owner unlink in sticky dir should fail"
        );
    }

    // ==================== nlink tracking on unlink tests ====================

    /// Tests that unlinking one hard link decrements nlink.
    #[test]
    fn unlink_decrements_nlink() {
        let archive = create_test_archive_with_links(
            &[("file.txt", b"data", 0o100644)],
            &[("file.txt", "link.txt")],
        );
        let mut state = BaleFsState::new(archive, false, 1000, 1000);

        let ino = *state.path_to_inode.get("file.txt").unwrap();
        assert_eq!(state.nlink_counts.get(&ino).copied(), Some(2));

        // Remove one link.
        state.remove_file(ROOT_INO, "link.txt", 0).unwrap();

        // nlink should now be 1.
        assert_eq!(
            state.nlink_counts.get(&ino).copied(),
            Some(1),
            "nlink should be 1 after removing one of two hard links"
        );
    }

    /// Tests that unlinking the last link fully removes the inode.
    #[test]
    fn unlink_last_link_removes_entry() {
        let archive = create_test_archive(&[("file.txt", b"data", 0o100644)]);
        let mut state = BaleFsState::new(archive, false, 1000, 1000);

        let ino = *state.path_to_inode.get("file.txt").unwrap();

        state.remove_file(ROOT_INO, "file.txt", 0).unwrap();

        // Inode should be fully gone.
        assert!(
            !state.inode_to_path.contains_key(&ino),
            "inode_to_path should not contain removed inode"
        );
        assert!(
            !state.path_to_inode.contains_key("file.txt"),
            "path_to_inode should not contain removed path"
        );
        assert!(
            !state.nlink_counts.contains_key(&ino),
            "nlink_counts should not contain removed inode"
        );
    }

    /// Tests that unlinking one hard link preserves the other path.
    #[test]
    fn unlink_hard_link_preserves_other_path() {
        let archive = create_test_archive_with_links(
            &[("file.txt", b"data", 0o100644)],
            &[("file.txt", "link.txt")],
        );
        let mut state = BaleFsState::new(archive, false, 1000, 1000);

        let ino = *state.path_to_inode.get("file.txt").unwrap();

        // Remove the original path, keeping the link.
        state.remove_file(ROOT_INO, "file.txt", 0).unwrap();

        // The link path should still exist.
        assert_eq!(
            state.path_to_inode.get("link.txt").copied(),
            Some(ino),
            "link.txt should still map to the same inode"
        );
        assert_eq!(
            state.inode_to_path.get(&ino).map(String::as_str),
            Some("link.txt"),
            "inode should now point to the remaining path"
        );

        // Should still be able to read the data from archive.
        assert!(
            state.archive.file("link.txt").is_ok(),
            "data should still be accessible via remaining link"
        );
    }

    // ==================== dir_depth tests ====================

    /// Tests that `dir_depth` returns 0 for the root directory.
    #[test]
    fn dir_depth_root_is_zero() {
        let archive = create_test_archive(&[]);
        let state = BaleFsState::new(archive, false, 1000, 1000);
        assert_eq!(state.dir_depth(ROOT_INO), 0);
    }

    /// Tests that `dir_depth` returns correct values for nested directories.
    #[test]
    fn dir_depth_nested_directories() {
        let archive = create_test_archive(&[
            ("a/", &[], 0o40755),
            ("a/b/", &[], 0o40755),
            ("a/b/c/", &[], 0o40755),
        ]);
        let state = BaleFsState::new(archive, false, 1000, 1000);

        let a_ino = *state.dir_inodes.get("a").unwrap();
        let ab_ino = *state.dir_inodes.get("a/b").unwrap();
        let abc_ino = *state.dir_inodes.get("a/b/c").unwrap();

        assert_eq!(state.dir_depth(a_ino), 1);
        assert_eq!(state.dir_depth(ab_ino), 2);
        assert_eq!(state.dir_depth(abc_ino), 3);
    }
}
