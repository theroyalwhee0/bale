//! BaleFs FUSE filesystem implementation.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::Path;
use std::sync::Mutex;
use std::time::SystemTime;

use fuser::{FileType, MountOption, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, Request};

use nix::libc;

use crate::fuse::{DIR_INO_START, FILE_INO_START, FuseDirEntry, ROOT_INO, TTL};
use crate::{ArchiveRead, ArchiveWriter, BaleError, CentralDirectoryHeader, EntryKind};

/// FUSE filesystem backed by a bale archive.
///
/// This struct wraps an `ArchiveWriter` and provides FUSE filesystem operations.
/// The filesystem can be mounted read-only or read-write.
pub struct BaleFs {
    /// Mutable filesystem state protected by a mutex.
    state: Mutex<BaleFsState>,
}

/// Internal state for the BaleFs filesystem.
struct BaleFsState {
    /// The underlying archive.
    archive: ArchiveWriter,
    /// Whether the filesystem is mounted read-only.
    read_only: bool,
    /// User ID for all files/directories.
    uid: u32,
    /// Group ID for all files/directories.
    gid: u32,
    /// Time when the filesystem was mounted.
    mount_time: SystemTime,

    /// Map from directory path to its inode.
    dir_inodes: HashMap<String, u64>,
    /// Map from directory inode to its contents.
    dir_contents: HashMap<u64, Vec<FuseDirEntry>>,

    /// Map from file/symlink inode to archive path.
    inode_to_path: HashMap<u64, String>,
    /// Map from archive path to file/symlink inode.
    path_to_inode: HashMap<String, u64>,

    /// Modified data for files (path -> new content).
    /// Used for write support when not read-only.
    modified_data: HashMap<String, Vec<u8>>,

    /// Next available inode for files.
    next_file_ino: u64,
    /// Next available inode for directories.
    next_dir_ino: u64,
}

impl BaleFs {
    /// Creates a new BaleFs from an archive path.
    ///
    /// Opens the archive and builds the directory tree from its entries.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the bale archive
    /// * `read_only` - Whether to mount read-only
    ///
    /// # Errors
    ///
    /// Returns an error if the archive cannot be opened.
    pub fn new(path: impl AsRef<Path>, read_only: bool) -> Result<Self, BaleError> {
        let archive = ArchiveWriter::open(path)?;
        let uid = {
            #[cfg(unix)]
            {
                nix::unistd::getuid().as_raw()
            }
            #[cfg(not(unix))]
            {
                0
            }
        };
        let gid = {
            #[cfg(unix)]
            {
                nix::unistd::getgid().as_raw()
            }
            #[cfg(not(unix))]
            {
                0
            }
        };

        let mut state = BaleFsState {
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

        Ok(Self {
            state: Mutex::new(state),
        })
    }

    /// Mounts the filesystem at the given mount point.
    ///
    /// This method blocks until the filesystem is unmounted (via `fusermount -u`
    /// or Ctrl+C if running in foreground).
    ///
    /// # Arguments
    ///
    /// * `mount_point` - Directory to mount the filesystem at
    /// * `allow_root` - Allow root to access the mount
    /// * `allow_other` - Allow other users to access the mount
    ///
    /// # Errors
    ///
    /// Returns an error if mounting fails.
    pub fn mount(
        self,
        mount_point: impl AsRef<Path>,
        allow_root: bool,
        allow_other: bool,
    ) -> Result<(), BaleError> {
        let read_only = self.state.lock().map_or(true, |s| s.read_only);

        let mut options = vec![
            MountOption::FSName("bale".to_string()),
            MountOption::DefaultPermissions,
        ];

        if read_only {
            options.push(MountOption::RO);
        } else {
            options.push(MountOption::RW);
        }

        if allow_root {
            options.push(MountOption::AllowRoot);
        }

        if allow_other {
            options.push(MountOption::AllowOther);
        }

        fuser::mount2(self, mount_point, &options)?;
        Ok(())
    }

    /// Mounts the filesystem in the background and returns a session handle.
    ///
    /// Unlike `mount()`, this method returns immediately with a `BackgroundSession`
    /// that keeps the filesystem mounted. The filesystem is unmounted when the
    /// session is dropped.
    ///
    /// # Arguments
    ///
    /// * `mount_point` - Directory to mount the filesystem at
    /// * `allow_root` - Allow root to access the mount
    /// * `allow_other` - Allow other users to access the mount
    ///
    /// # Errors
    ///
    /// Returns an error if mounting fails.
    pub fn mount_background(
        self,
        mount_point: impl AsRef<Path>,
        allow_root: bool,
        allow_other: bool,
    ) -> Result<fuser::BackgroundSession, BaleError> {
        let read_only = self.state.lock().map_or(true, |s| s.read_only);

        let mut options = vec![
            MountOption::FSName("bale".to_string()),
            MountOption::DefaultPermissions,
        ];

        if read_only {
            options.push(MountOption::RO);
        } else {
            options.push(MountOption::RW);
        }

        if allow_root {
            options.push(MountOption::AllowRoot);
        }

        if allow_other {
            options.push(MountOption::AllowOther);
        }

        let session = fuser::spawn_mount2(self, mount_point, &options)?;
        Ok(session)
    }
}

impl BaleFsState {
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
    fn split_path(path: &str) -> Option<(&str, &str)> {
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
    fn get_attr(&self, ino: u64, kind: FileType) -> fuser::FileAttr {
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
    fn get_attr_for_file(
        &self,
        ino: u64,
        kind: FileType,
        header: &CentralDirectoryHeader,
    ) -> fuser::FileAttr {
        let mode = header.external_attrs.get() >> 16;
        let perm = (mode & 0o777) as u16;
        let size = header.uncompressed_size.get() as u64;

        fuser::FileAttr {
            ino,
            size,
            blocks: size.div_ceil(512),
            atime: self.mount_time,
            mtime: self.mount_time, // TODO: Use entry mtime from DOS timestamp
            ctime: self.mount_time,
            crtime: self.mount_time,
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
}

impl fuser::Filesystem for BaleFs {
    /// Looks up a directory entry by name.
    fn lookup(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        let name_str = match name.to_str() {
            Some(s) => s,
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };

        // Find parent directory contents.
        let contents = match state.dir_contents.get(&parent) {
            Some(c) => c,
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };

        // Find the entry.
        for entry in contents {
            if entry.name == name_str {
                let attr = state.get_attr(entry.ino, entry.kind);
                reply.entry(&TTL, &attr, 0);
                return;
            }
        }

        reply.error(libc::ENOENT);
    }

    /// Gets file attributes.
    fn getattr(&mut self, _req: &Request<'_>, ino: u64, _fh: Option<u64>, reply: ReplyAttr) {
        let state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        // Root directory.
        if ino == ROOT_INO {
            let attr = state.get_attr(ROOT_INO, FileType::Directory);
            reply.attr(&TTL, &attr);
            return;
        }

        // Check if it's a directory.
        if state.dir_contents.contains_key(&ino) {
            let attr = state.get_attr(ino, FileType::Directory);
            reply.attr(&TTL, &attr);
            return;
        }

        // Check if it's a file/symlink.
        if let Some(path) = state.inode_to_path.get(&ino) {
            // Determine file type from archive.
            if let Some((header, _, _)) = state.archive.find_entry_with_path(path) {
                let kind = EntryKind::from_mode(header.external_attrs.get() >> 16);
                let file_type = match kind {
                    EntryKind::Symlink => FileType::Symlink,
                    _ => FileType::RegularFile,
                };
                let attr = state.get_attr_for_file(ino, file_type, header);
                reply.attr(&TTL, &attr);
                return;
            }
        }

        reply.error(libc::ENOENT);
    }

    /// Reads directory contents.
    fn readdir(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        let state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        let contents = match state.dir_contents.get(&ino) {
            Some(c) => c,
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };

        // Standard . and .. entries.
        let mut entries: Vec<(u64, FileType, &str)> = vec![
            (ino, FileType::Directory, "."),
            (ROOT_INO, FileType::Directory, ".."),
        ];

        for entry in contents {
            entries.push((entry.ino, entry.kind, &entry.name));
        }

        for (i, (entry_ino, kind, name)) in entries.iter().enumerate().skip(offset as usize) {
            let buffer_full = reply.add(*entry_ino, (i + 1) as i64, *kind, name);
            if buffer_full {
                break;
            }
        }

        reply.ok();
    }

    /// Reads file data.
    fn read(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        let state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        let path = match state.inode_to_path.get(&ino) {
            Some(p) => p,
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };

        // Check for modified data first.
        if let Some(data) = state.modified_data.get(path) {
            let start = offset as usize;
            let end = (offset as usize + size as usize).min(data.len());
            if start < data.len() {
                reply.data(&data[start..end]);
            } else {
                reply.data(&[]);
            }
            return;
        }

        // Read from archive.
        let data = match state.archive.file(path) {
            Ok(f) => f.data(),
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        let start = offset as usize;
        let end = (offset as usize + size as usize).min(data.len());
        if start < data.len() {
            reply.data(&data[start..end]);
        } else {
            reply.data(&[]);
        }
    }

    /// Reads symlink target.
    fn readlink(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyData) {
        let state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        let path = match state.inode_to_path.get(&ino) {
            Some(p) => p,
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };

        match state.archive.symlink(path) {
            Ok(s) => reply.data(s.target_bytes()),
            Err(_) => reply.error(libc::EIO),
        }
    }
}
