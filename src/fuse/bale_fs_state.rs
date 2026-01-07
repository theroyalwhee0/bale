//! Internal state for the BaleFs filesystem.

use std::collections::HashMap;
use std::time::SystemTime;

use fuser::FileType;
use nix::libc;

use crate::fuse::{DIR_INO_START, FILE_INO_START, FuseDirEntry, ROOT_INO};
use crate::{
    ArchiveRead, ArchiveWrite, ArchiveWriter, CentralDirectoryHeader, DosDateTime, EntryKind,
};

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
                0o755
            } else {
                0o644
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
        let perm = (mode & 0o777) as u16;
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
            perm: if perm == 0 { 0o644 } else { perm },
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
                if mode == 0 { 0o644 } else { mode }
            })
            .unwrap_or(0o644)
    }

    /// Syncs all modified files to the archive.
    pub(super) fn sync_modified_to_archive(&mut self) -> Result<(), i32> {
        // Collect paths to avoid borrow issues.
        let paths: Vec<String> = self.modified_data.keys().cloned().collect();

        for path in paths {
            let mode = self.get_file_mode(&path);
            let data = self.modified_data.get(&path).unwrap().clone();
            self.archive
                .add_entry(&path, &data, mode)
                .map_err(|_| libc::EIO)?;
        }

        self.archive.sync().map_err(|_| libc::EIO)?;
        Ok(())
    }

    /// Creates a new directory.
    ///
    /// Returns the inode of the new directory and its attributes.
    pub(super) fn create_directory(
        &mut self,
        parent_ino: u64,
        name: &str,
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
        let mode = 0o40755u32; // directory with rwxr-xr-x
        self.archive
            .add_entry(&dir_path, &[], mode)
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
}
