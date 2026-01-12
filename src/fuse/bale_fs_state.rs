//! Internal state for the BaleFs filesystem.

use std::collections::HashMap;
use std::time::SystemTime;

use fuser::FileType;
use nix::libc;
use nix::sys::stat::{Mode, SFlag};

use crate::fuse::{DIR_INO_START, FILE_INO_START, FuseDirEntry, ROOT_INO};
use crate::{
    ArchiveRead, ArchiveWrite, ArchiveWriter, CentralDirectoryHeader, DosDateTime, EntryKind,
};

/// Default permission bits for regular files (rw-r--r--).
pub(super) const DEFAULT_FILE_PERM: u32 = 0o644;

/// Default permission bits for directories (rwxr-xr-x).
pub(super) const DEFAULT_DIR_PERM: u32 = 0o755;

/// Bitmask to extract permission bits from a mode value.
pub(super) const PERM_MASK: u32 = 0o777;

/// Default file mode: regular file with default permissions.
const DEFAULT_FILE_MODE: u32 = SFlag::S_IFREG.bits() | DEFAULT_FILE_PERM;

/// Default directory mode: directory with default permissions.
const DEFAULT_DIR_MODE: u32 = SFlag::S_IFDIR.bits() | DEFAULT_DIR_PERM;

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

    /// Next available inode for files.
    next_file_ino: u64,
    /// Next available inode for directories.
    next_dir_ino: u64,
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
            next_file_ino: FILE_INO_START,
            next_dir_ino: DIR_INO_START,
        };
        state.build_directory_tree();
        state
    }

    /// Builds the directory tree from archive entries.
    ///
    /// Iterates through all archive entries and creates:
    /// - Directory inodes for all directories (explicit and implicit)
    /// - File/symlink inodes for all files and symlinks
    /// - Directory content listings
    fn build_directory_tree(&mut self) {
        // Root directory always exists.
        self.dir_inodes.insert(String::new(), ROOT_INO);
        self.dir_contents.insert(ROOT_INO, Vec::new());

        // Collect all paths first to avoid borrow issues.
        // Skip entries with invalid UTF-8 paths.
        let entries: Vec<(String, EntryKind)> = self
            .archive
            .iter_entries()
            .filter_map(|(header, path_bytes)| {
                let trimmed = &path_bytes[..path_bytes
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(path_bytes.len())];
                let path = std::str::from_utf8(trimmed).ok()?.to_owned();
                let kind = EntryKind::from_mode(header.external_attrs.get() >> 16);
                Some((path, kind))
            })
            .collect();

        for (path, kind) in entries {
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

                        // Add to parent directory.
                        if let Some((parent, name)) = Self::split_path(&dir_path)
                            && let Some(&parent_ino) = self.dir_inodes.get(parent)
                            && let Some(contents) = self.dir_contents.get_mut(&parent_ino)
                        {
                            contents.push(FuseDirEntry::directory(name, ino));
                        }
                    }
                }
                EntryKind::File | EntryKind::Symlink => {
                    let ino = self.next_file_ino;
                    self.next_file_ino += 1;

                    self.inode_to_path.insert(ino, path.clone());
                    self.path_to_inode.insert(path.clone(), ino);

                    // Add to parent directory.
                    if let Some((parent, name)) = Self::split_path(&path)
                        && let Some(&parent_ino) = self.dir_inodes.get(parent)
                        && let Some(contents) = self.dir_contents.get_mut(&parent_ino)
                    {
                        let entry = if kind == EntryKind::Symlink {
                            FuseDirEntry::symlink(name, ino)
                        } else {
                            FuseDirEntry::file(name, ino)
                        };
                        contents.push(entry);
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
        fuser::FileAttr {
            ino,
            size: 0,
            blocks: 0,
            atime: self.mount_time,
            mtime: self.mount_time,
            ctime: self.mount_time,
            crtime: self.mount_time,
            kind,
            perm: if kind == FileType::Directory {
                DEFAULT_DIR_PERM as u16
            } else {
                DEFAULT_FILE_PERM as u16
            },
            nlink: 1,
            uid: self.uid,
            gid: self.gid,
            rdev: 0,
            blksize: 4096,
            flags: 0,
        }
    }

    /// Creates file attributes from archive entry header.
    pub(super) fn get_attr_for_file(
        &self,
        ino: u64,
        kind: FileType,
        header: &CentralDirectoryHeader,
    ) -> fuser::FileAttr {
        let mode = header.external_attrs.get() >> 16;
        let perm = (mode & PERM_MASK) as u16;
        let size = header.uncompressed_size.get() as u64;
        let mtime = DosDateTime::from_date_time_parts(header.mod_date.get(), header.mod_time.get())
            .to_system_time_or_epoch();

        fuser::FileAttr {
            ino,
            size,
            blocks: size.div_ceil(512),
            atime: mtime,
            mtime,
            ctime: mtime,
            crtime: mtime,
            kind,
            perm: if perm == 0 {
                DEFAULT_FILE_PERM as u16
            } else {
                perm
            },
            nlink: 1,
            uid: self.uid,
            gid: self.gid,
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

    /// Gets the mode (permissions) for a file from the archive.
    pub(super) fn get_file_mode(&self, path: &str) -> u32 {
        self.archive
            .find_entry_with_path(path)
            .map(|(header, _, _)| {
                let mode = header.external_attrs.get() >> 16;
                if mode == 0 { DEFAULT_FILE_MODE } else { mode }
            })
            .unwrap_or(DEFAULT_FILE_MODE)
    }

    /// Syncs all modified files to the archive.
    pub(super) fn sync_modified_to_archive(&mut self) -> Result<(), i32> {
        // Collect paths to avoid borrow issues.
        let paths: Vec<String> = self.modified_data.keys().cloned().collect();

        log::trace!("sync_modified_to_archive: {} modified files", paths.len());

        for path in paths {
            let mode = self.get_file_mode(&path);
            let data = self.modified_data.get(&path).unwrap().clone();
            // Delete existing entry first to avoid duplicates (create() adds
            // an initial entry, so we must remove it before re-adding with
            // the final content).
            self.archive.delete(&path);
            self.archive
                .add_entry(&path, &data, mode)
                .map_err(|_| libc::EIO)?;
        }

        log::trace!("sync_modified_to_archive: calling archive.sync()");
        self.archive.sync().map_err(|_| libc::EIO)?;
        self.modified_data.clear();
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

        // Add directory entry to archive (path ending with /).
        let dir_path = format!("{}/", full_path);
        let archive_mode = SFlag::S_IFDIR.bits() | mode.bits();
        self.archive
            .add_entry(&dir_path, &[], archive_mode)
            .map_err(|_| libc::EIO)?;

        let attr = self.get_attr(ino, FileType::Directory);
        Ok((ino, attr))
    }

    /// Removes an empty directory.
    pub(super) fn remove_directory(&mut self, parent_ino: u64, name: &str) -> Result<(), i32> {
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

        // Remove from dir_inodes and dir_contents.
        self.dir_inodes.remove(&full_path);
        self.dir_contents.remove(&dir_ino);

        // Remove from parent's contents.
        if let Some(contents) = self.dir_contents.get_mut(&parent_ino) {
            contents.retain(|e| e.name != name);
        }

        // Remove from archive.
        let dir_path = format!("{}/", full_path);
        let _ = self.archive.delete(&dir_path);

        Ok(())
    }

    /// Gets the path for a directory inode.
    fn get_dir_path(&self, ino: u64) -> String {
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

    /// Creates a new file.
    ///
    /// Returns the inode of the new file and its attributes.
    pub(super) fn create_file(
        &mut self,
        parent_ino: u64,
        name: &str,
        mode: Mode,
    ) -> Result<(u64, fuser::FileAttr), i32> {
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
                DEFAULT_FILE_PERM as u16
            } else {
                perm
            },
            nlink: 1,
            uid: self.uid,
            gid: self.gid,
            rdev: 0,
            blksize: 4096,
            flags: 0,
        };

        Ok((ino, attr))
    }

    /// Removes a file.
    pub(super) fn remove_file(&mut self, parent_ino: u64, name: &str) -> Result<(), i32> {
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

        // Remove from path mappings.
        self.inode_to_path.remove(&file_ino);
        self.path_to_inode.remove(&full_path);

        // Remove from parent's contents.
        if let Some(contents) = self.dir_contents.get_mut(&parent_ino) {
            contents.retain(|e| e.name != name);
        }

        // Remove from modified_data if present.
        self.modified_data.remove(&full_path);

        // Delete from archive.
        let _ = self.archive.delete(&full_path);

        Ok(())
    }

    /// Renames/moves a file or directory.
    pub(super) fn rename_entry(
        &mut self,
        old_parent_ino: u64,
        old_name: &str,
        new_parent_ino: u64,
        new_name: &str,
    ) -> Result<(), i32> {
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

        // Check if source is a file or directory.
        let is_file = self.path_to_inode.contains_key(&old_path);
        let is_dir = self.dir_inodes.contains_key(&old_path);

        if !is_file && !is_dir {
            return Err(libc::ENOENT);
        }

        // Check destination doesn't exist (or handle overwrite for files).
        if self.dir_inodes.contains_key(&new_path) {
            return Err(libc::EEXIST);
        }
        if self.path_to_inode.contains_key(&new_path) {
            // Destination file exists - remove it first.
            if is_dir {
                // Can't overwrite file with directory.
                return Err(libc::ENOTDIR);
            }
            // Remove destination file.
            let dest_ino = *self.path_to_inode.get(&new_path).unwrap();
            self.inode_to_path.remove(&dest_ino);
            self.path_to_inode.remove(&new_path);
            self.modified_data.remove(&new_path);
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

            // Update parent directory contents.
            if let Some(contents) = self.dir_contents.get_mut(&old_parent_ino) {
                contents.retain(|e| e.name != old_name);
            }
            if let Some(contents) = self.dir_contents.get_mut(&new_parent_ino) {
                contents.push(FuseDirEntry::file(new_name, ino));
            }

            // Update archive: copy data from old path to new, delete old.
            if let Ok(file) = self.archive.file(&old_path) {
                let data = file.data().to_vec();
                let mode = self.get_file_mode(&old_path);
                let _ = self.archive.add_entry(&new_path, &data, mode);
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
                    self.modified_data.insert(new_p, data);
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
                self.dir_inodes.insert(new_p, ino);
            }

            // Update archive: add new dir entry, remove old.
            let old_dir_path = format!("{}/", old_path);
            let new_dir_path = format!("{}/", new_path);
            let _ = self.archive.add_entry(&new_dir_path, &[], DEFAULT_DIR_MODE);
            let _ = self.archive.delete(&old_dir_path);
        }

        Ok(())
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
        let (header, _path) = entries
            .iter()
            .find(|(_, p)| p.starts_with(b"private/"))
            .unwrap();

        // Mode is stored in upper 16 bits of external_attrs.
        let mode = header.external_attrs.get() >> 16;

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
}
