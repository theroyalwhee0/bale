//! BaleFs FUSE filesystem implementation.

use std::ffi::OsStr;
use std::path::Path;
use std::sync::Mutex;
use std::time::SystemTime;

use fuser::{
    FileType, MountOption, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory, ReplyEmpty,
    ReplyEntry, ReplyStatfs, ReplyWrite, Request, TimeOrNow,
};

use nix::libc;
use nix::sys::stat::Mode;

use crate::fuse::bale_fs_state::{BaleFsState, DEFAULT_FILE_PERM, PERM_MASK};
use crate::fuse::{ROOT_INO, TTL};
use crate::{ArchiveRead, ArchiveWriter, BaleError, DosDateTime, EntryKind};

/// FUSE filesystem backed by a bale archive.
///
/// This struct wraps an `ArchiveWriter` and provides FUSE filesystem operations.
/// The filesystem can be mounted read-only or read-write.
pub struct BaleFs {
    /// Mutable filesystem state protected by a mutex.
    state: Mutex<BaleFsState>,
    /// Archive filename for FSName mount option.
    archive_name: String,
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
        let archive_name = path
            .as_ref()
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("archive")
            .to_string();
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

        let state = BaleFsState::new(archive, read_only, uid, gid);

        Ok(Self {
            state: Mutex::new(state),
            archive_name,
        })
    }

    /// Mounts the filesystem at the given mount point.
    ///
    /// Returns a `BackgroundSession` that keeps the filesystem mounted.
    /// The filesystem is unmounted when the session is dropped.
    ///
    /// For a blocking mount, call `.join()` on the returned session.
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
    ) -> Result<fuser::BackgroundSession, BaleError> {
        let read_only = self.state.lock().map_or(true, |s| s.read_only);

        let mut options = vec![
            MountOption::FSName(format!("bale:{}", self.archive_name)),
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
                reply.error(libc::EINVAL);
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
                let attr = match entry.kind {
                    FileType::Directory => state.get_attr(entry.ino, entry.kind),
                    FileType::RegularFile | FileType::Symlink => {
                        // Look up file attributes from archive.
                        if let Some(path) = state.inode_to_path.get(&entry.ino)
                            && let Some((header, _, _)) = state.archive.find_entry_with_path(path)
                        {
                            let mut attr = state.get_attr_for_file(entry.ino, entry.kind, header);
                            // Override size if file has been modified.
                            if let Some(data) = state.modified_data.get(path) {
                                attr.size = data.len() as u64;
                                attr.blocks = attr.size.div_ceil(512);
                            }
                            attr
                        } else {
                            state.get_attr(entry.ino, entry.kind)
                        }
                    }
                    _ => state.get_attr(entry.ino, entry.kind),
                };
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
                let mut attr = state.get_attr_for_file(ino, file_type, header);

                // Override size if file has been modified.
                if let Some(data) = state.modified_data.get(path) {
                    attr.size = data.len() as u64;
                    attr.blocks = attr.size.div_ceil(512);
                }

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
        let parent_ino = state.get_parent_inode(ino);
        let mut entries: Vec<(u64, FileType, &str)> = vec![
            (ino, FileType::Directory, "."),
            (parent_ino, FileType::Directory, ".."),
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

    /// Writes data to a file.
    fn write(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        data: &[u8],
        _write_flags: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyWrite,
    ) {
        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        // Check read-only flag.
        if state.read_only {
            reply.error(libc::EROFS);
            return;
        }

        // Get path from inode.
        let path = match state.inode_to_path.get(&ino) {
            Some(p) => p.clone(),
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };

        // Load file into modified_data if not already there.
        let buffer = match state.load_into_modified(&path) {
            Ok(b) => b,
            Err(e) => {
                reply.error(e);
                return;
            }
        };

        // Extend buffer if needed.
        let start = offset as usize;
        let end = start + data.len();
        if end > buffer.len() {
            buffer.resize(end, 0);
        }

        // Copy data into buffer.
        buffer[start..end].copy_from_slice(data);

        reply.written(data.len() as u32);
    }

    /// Sets file attributes.
    #[allow(clippy::too_many_arguments)]
    fn setattr(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _mode: Option<u32>,
        _uid: Option<u32>,
        _gid: Option<u32>,
        size: Option<u64>,
        _atime: Option<TimeOrNow>,
        _mtime: Option<TimeOrNow>,
        _ctime: Option<SystemTime>,
        _fh: Option<u64>,
        _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>,
        _bkuptime: Option<SystemTime>,
        _flags: Option<u32>,
        reply: ReplyAttr,
    ) {
        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        // Check read-only for size changes.
        if size.is_some() && state.read_only {
            reply.error(libc::EROFS);
            return;
        }

        // Get path from inode.
        let path = match state.inode_to_path.get(&ino) {
            Some(p) => p.clone(),
            None => {
                // Might be a directory - just return current attrs.
                if state.dir_contents.contains_key(&ino) {
                    let attr = state.get_attr(ino, FileType::Directory);
                    reply.attr(&TTL, &attr);
                    return;
                }
                reply.error(libc::ENOENT);
                return;
            }
        };

        // Handle truncation.
        if let Some(new_size) = size {
            let buffer = match state.load_into_modified(&path) {
                Ok(b) => b,
                Err(e) => {
                    reply.error(e);
                    return;
                }
            };
            buffer.resize(new_size as usize, 0);
        }

        // Return updated attributes.
        let size = state
            .modified_data
            .get(&path)
            .map(|d| d.len() as u64)
            .or_else(|| {
                state
                    .archive
                    .find_entry_with_path(&path)
                    .map(|(h, _, _)| h.uncompressed_size.get() as u64)
            })
            .unwrap_or(0);

        let mtime = state
            .archive
            .find_entry_with_path(&path)
            .map(|(h, _, _)| {
                DosDateTime::from_date_time_parts(h.mod_date.get(), h.mod_time.get())
                    .to_system_time_or_epoch()
            })
            .unwrap_or(state.mount_time);

        let mode = state.get_file_mode(&path);
        let perm = (mode & PERM_MASK) as u16;

        let attr = fuser::FileAttr {
            ino,
            size,
            blocks: size.div_ceil(512),
            atime: mtime,
            mtime,
            ctime: mtime,
            crtime: mtime,
            kind: FileType::RegularFile,
            perm: if perm == 0 {
                DEFAULT_FILE_PERM as u16
            } else {
                perm
            },
            nlink: 1,
            uid: state.uid,
            gid: state.gid,
            rdev: 0,
            blksize: 4096,
            flags: 0,
        };

        reply.attr(&TTL, &attr);
    }

    /// Creates a new directory.
    fn mkdir(
        &mut self,
        _req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        if state.read_only {
            reply.error(libc::EROFS);
            return;
        }

        let name = match name.to_str() {
            Some(n) => n,
            None => {
                reply.error(libc::EINVAL);
                return;
            }
        };

        let mode = Mode::from_bits_truncate(mode);
        match state.create_directory(parent, name, mode) {
            Ok((ino, attr)) => reply.entry(&TTL, &attr, ino),
            Err(e) => reply.error(e),
        }
    }

    /// Removes an empty directory.
    fn rmdir(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        if state.read_only {
            reply.error(libc::EROFS);
            return;
        }

        let name = match name.to_str() {
            Some(n) => n,
            None => {
                reply.error(libc::EINVAL);
                return;
            }
        };

        match state.remove_directory(parent, name) {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(e),
        }
    }

    /// Creates a new file.
    fn create(
        &mut self,
        _req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        mode: u32,
        _umask: u32,
        _flags: i32,
        reply: ReplyCreate,
    ) {
        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        if state.read_only {
            reply.error(libc::EROFS);
            return;
        }

        let name = match name.to_str() {
            Some(n) => n,
            None => {
                reply.error(libc::EINVAL);
                return;
            }
        };

        let mode = Mode::from_bits_truncate(mode);
        match state.create_file(parent, name, mode) {
            Ok((ino, attr)) => {
                // Use inode as file handle for simplicity.
                reply.created(&TTL, &attr, ino, ino, 0);
            }
            Err(e) => reply.error(e),
        }
    }

    /// Removes a file.
    fn unlink(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        if state.read_only {
            reply.error(libc::EROFS);
            return;
        }

        let name = match name.to_str() {
            Some(n) => n,
            None => {
                reply.error(libc::EINVAL);
                return;
            }
        };

        match state.remove_file(parent, name) {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(e),
        }
    }

    /// Renames/moves a file or directory.
    fn rename(
        &mut self,
        _req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        newparent: u64,
        newname: &OsStr,
        _flags: u32,
        reply: ReplyEmpty,
    ) {
        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        if state.read_only {
            reply.error(libc::EROFS);
            return;
        }

        let old_name = match name.to_str() {
            Some(n) => n,
            None => {
                reply.error(libc::EINVAL);
                return;
            }
        };

        let new_name = match newname.to_str() {
            Some(n) => n,
            None => {
                reply.error(libc::EINVAL);
                return;
            }
        };

        match state.rename_entry(parent, old_name, newparent, new_name) {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(e),
        }
    }

    /// Syncs file data on close.
    fn flush(
        &mut self,
        _req: &Request<'_>,
        _ino: u64,
        _fh: u64,
        _lock_owner: u64,
        reply: ReplyEmpty,
    ) {
        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        if state.read_only {
            reply.ok();
            return;
        }

        match state.sync_modified_to_archive() {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(e),
        }
    }

    /// Returns filesystem statistics.
    fn statfs(&mut self, _req: &Request<'_>, _ino: u64, reply: ReplyStatfs) {
        let state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        let file_count = state.path_to_inode.len() as u64;
        let dir_count = state.dir_inodes.len() as u64;

        // Return basic stats. Block size matches our alignment.
        reply.statfs(
            0,                      // blocks (unknown for archive)
            0,                      // bfree
            0,                      // bavail
            file_count + dir_count, // files (inodes)
            0,                      // ffree
            4096,                   // bsize (block size)
            256,                    // namelen (our max path component)
            0,                      // frsize (fragment size)
        );
    }

    /// Syncs file data to the archive.
    fn fsync(
        &mut self,
        _req: &Request<'_>,
        _ino: u64,
        _fh: u64,
        _datasync: bool,
        reply: ReplyEmpty,
    ) {
        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                reply.error(libc::EIO);
                return;
            }
        };

        if state.read_only {
            reply.ok();
            return;
        }

        match state.sync_modified_to_archive() {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(e),
        }
    }

    /// Called when the filesystem is unmounted.
    fn destroy(&mut self) {
        // Sync any remaining modified data.
        if let Ok(mut state) = self.state.lock()
            && !state.read_only
        {
            let _ = state.sync_modified_to_archive();
        }
    }
}
