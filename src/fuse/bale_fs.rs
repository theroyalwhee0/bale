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

use crate::fuse::bale_fs_state::{BaleFsState, DEFAULT_FILE_PERM};
use crate::fuse::virtual_dir::VirtualDir;
use crate::fuse::{ROOT_INO, TTL};
use crate::{ArchiveRead, ArchiveWriter, BaleError, EntryKind};

/// Maximum file size allowed for truncation (4GB).
///
/// This prevents OOM crashes from huge ftruncate requests. The limit is
/// chosen to fit in memory while still allowing large files. Files in the
/// archive can be larger if added directly, but cannot be created or
/// extended beyond this size via FUSE.
const MAX_FILE_SIZE: u64 = 4 * 1024 * 1024 * 1024;

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

        let canonical = mount_point.as_ref().canonicalize()?;
        if let Ok(mut state) = self.state.lock() {
            state.set_mount_point(canonical);
        }

        let session = fuser::spawn_mount2(self, mount_point, &options)?;
        Ok(session)
    }
}

impl BaleFs {
    /// Creates file attributes for the virtual `.bale/` directory.
    fn virtual_dir_attr(state: &BaleFsState) -> fuser::FileAttr {
        fuser::FileAttr {
            ino: state.virtual_dir.dir_ino(),
            size: 0,
            blocks: 0,
            atime: state.mount_time,
            mtime: state.mount_time,
            ctime: state.mount_time,
            crtime: state.mount_time,
            kind: FileType::Directory,
            perm: 0o555,
            nlink: 2,
            uid: state.uid,
            gid: state.gid,
            rdev: 0,
            blksize: 4096,
            flags: 0,
        }
    }

    /// Creates file attributes for a virtual metadata file.
    fn virtual_file_attr(state: &BaleFsState, ino: u64, size: u64) -> fuser::FileAttr {
        fuser::FileAttr {
            ino,
            size,
            blocks: size.div_ceil(512),
            atime: state.mount_time,
            mtime: state.mount_time,
            ctime: state.mount_time,
            crtime: state.mount_time,
            kind: FileType::RegularFile,
            perm: 0o444,
            nlink: 1,
            uid: state.uid,
            gid: state.gid,
            rdev: 0,
            blksize: 4096,
            flags: 0,
        }
    }

    /// Creates file attributes for a virtual `.bale` symlink.
    fn virtual_symlink_attr(state: &BaleFsState, ino: u64, target_len: u64) -> fuser::FileAttr {
        fuser::FileAttr {
            ino,
            size: target_len,
            blocks: 0,
            atime: state.mount_time,
            mtime: state.mount_time,
            ctime: state.mount_time,
            crtime: state.mount_time,
            kind: FileType::Symlink,
            perm: 0o777,
            nlink: 1,
            uid: state.uid,
            gid: state.gid,
            rdev: 0,
            blksize: 4096,
            flags: 0,
        }
    }
}

impl fuser::Filesystem for BaleFs {
    /// Looks up a directory entry by name.
    fn lookup(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let mut state = match self.state.lock() {
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

        // Handle ".bale" lookup in any directory (hidden — not in readdir).
        if name_str == ".bale" && state.dir_contents.contains_key(&parent) {
            if parent == ROOT_INO {
                // Root: ".bale" is the virtual directory.
                let attr = Self::virtual_dir_attr(&state);
                reply.entry(&TTL, &attr, 0);
            } else {
                // Non-root: ".bale" is a symlink to the root .bale/.
                let depth = state.dir_depth(parent);
                let target = VirtualDir::symlink_target(depth);
                let sym_ino = state.virtual_dir.symlink_ino_for(parent, depth);
                let attr = Self::virtual_symlink_attr(&state, sym_ino, target.len() as u64);
                reply.entry(&TTL, &attr, 0);
            }
            return;
        }

        // Handle lookup inside the .bale/ virtual directory.
        if parent == state.virtual_dir.dir_ino() {
            if let Some((ino, _kind)) = state.virtual_dir.lookup(name_str) {
                let content = state
                    .virtual_dir
                    .file_content(ino, &state.archive)
                    .unwrap_or_default();
                let attr = Self::virtual_file_attr(&state, ino, content.len() as u64);
                reply.entry(&TTL, &attr, 0);
            } else {
                reply.error(libc::ENOENT);
            }
            return;
        }

        // Validate filename component using safename rules.
        if let Err(e) = BaleFsState::validate_name(name_str) {
            reply.error(e);
            return;
        }

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
                            let mut attr =
                                state.get_attr_for_file(entry.ino, entry.kind, header, path);
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

        // Virtual inode handling.
        if state.virtual_dir.is_virtual(ino) {
            if ino == state.virtual_dir.dir_ino() {
                let attr = Self::virtual_dir_attr(&state);
                reply.attr(&TTL, &attr);
            } else if let Some(depth) = state.virtual_dir.symlink_depth(ino) {
                let target = VirtualDir::symlink_target(depth);
                let attr = Self::virtual_symlink_attr(&state, ino, target.len() as u64);
                reply.attr(&TTL, &attr);
            } else {
                // Virtual metadata file.
                let content = state
                    .virtual_dir
                    .file_content(ino, &state.archive)
                    .unwrap_or_default();
                let attr = Self::virtual_file_attr(&state, ino, content.len() as u64);
                reply.attr(&TTL, &attr);
            }
            return;
        }

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
            if let Some((entry_row, _, _)) = state.archive.find_entry_with_path(path) {
                let kind = entry_row.kind();
                let file_type = match kind {
                    EntryKind::Symlink => FileType::Symlink,
                    _ => FileType::RegularFile,
                };
                let mut attr = state.get_attr_for_file(ino, file_type, entry_row, path);

                // Override size if file has been modified.
                if let Some(data) = state.modified_data.get(path) {
                    attr.size = data.len() as u64;
                    attr.blocks = attr.size.div_ceil(512);
                }

                // Override mode if it has been changed via chmod.
                if let Some(&mode) = state.modified_modes.get(path) {
                    attr.perm = mode.bits() as u16;
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

        // Handle readdir for the virtual .bale/ directory.
        if ino == state.virtual_dir.dir_ino() {
            let mut entries: Vec<(u64, FileType, &str)> = vec![
                (ino, FileType::Directory, "."),
                (ROOT_INO, FileType::Directory, ".."),
            ];
            for (vino, kind, name) in state.virtual_dir.readdir() {
                entries.push((vino, kind, name));
            }
            for (i, (entry_ino, kind, name)) in entries.iter().enumerate().skip(offset as usize) {
                let buffer_full = reply.add(*entry_ino, (i + 1) as i64, *kind, name);
                if buffer_full {
                    break;
                }
            }
            reply.ok();
            return;
        }

        let contents = match state.dir_contents.get(&ino) {
            Some(c) => c,
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };

        // Standard . and .. entries.
        // Note: .bale is NOT listed in readdir — it's hidden, only accessible via lookup.
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

        // Handle read for virtual metadata files.
        if state.virtual_dir.is_virtual(ino) {
            if let Some(content) = state.virtual_dir.file_content(ino, &state.archive) {
                let bytes = content.as_bytes();
                let start = offset as usize;
                let end = (start + size as usize).min(bytes.len());
                if start < bytes.len() {
                    reply.data(&bytes[start..end]);
                } else {
                    reply.data(&[]);
                }
            } else {
                reply.error(libc::ENOENT);
            }
            return;
        }

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

        // Virtual symlink — each directory has its own symlink inode
        // so we can return the correct depth-relative target.
        if let Some(depth) = state.virtual_dir.symlink_depth(ino) {
            let target = VirtualDir::symlink_target(depth);
            reply.data(target.as_bytes());
            return;
        }

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

        // Virtual entries are read-only.
        if state.virtual_dir.is_virtual(ino) {
            reply.error(libc::EACCES);
            return;
        }

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

        // Check size limit to prevent OOM crashes.
        let start = offset as usize;
        let end = start.saturating_add(data.len());
        if end as u64 > MAX_FILE_SIZE {
            reply.error(libc::EFBIG);
            return;
        }

        // Extend buffer if needed.
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
        mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        size: Option<u64>,
        _atime: Option<TimeOrNow>,
        mtime: Option<TimeOrNow>,
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

        // Virtual entries are read-only.
        if state.virtual_dir.is_virtual(ino) {
            reply.error(libc::EACCES);
            return;
        }

        // Check read-only for size, mode, or mtime changes.
        // chown is allowed even on read-only mounts (transient only).
        if (size.is_some() || mode.is_some() || mtime.is_some()) && state.read_only {
            reply.error(libc::EROFS);
            return;
        }

        // Get path from inode.
        let path = match state.inode_to_path.get(&ino) {
            Some(p) => p.clone(),
            None => {
                // Might be a directory.
                if state.dir_contents.contains_key(&ino) {
                    // Handle directory mode change.
                    if let Some(new_mode) = mode {
                        state.set_dir_mode(ino, Mode::from_bits_truncate(new_mode));
                    }
                    // Handle directory mtime change.
                    if let Some(new_mtime) = mtime {
                        let time = match new_mtime {
                            TimeOrNow::SpecificTime(t) => t,
                            TimeOrNow::Now => SystemTime::now(),
                        };
                        state.set_dir_mtime(ino, time);
                    }
                    // Handle directory chown (transient).
                    if let Some(new_uid) = uid {
                        let dir_path = state.get_dir_path(ino);
                        state.modified_uids.insert(dir_path, new_uid);
                    }
                    if let Some(new_gid) = gid {
                        let dir_path = state.get_dir_path(ino);
                        state.modified_gids.insert(dir_path, new_gid);
                    }
                    let attr = state.get_attr(ino, FileType::Directory);
                    reply.attr(&TTL, &attr);
                    return;
                }
                reply.error(libc::ENOENT);
                return;
            }
        };

        // Handle file/symlink chown (transient).
        if let Some(new_uid) = uid {
            state.modified_uids.insert(path.clone(), new_uid);
        }
        if let Some(new_gid) = gid {
            state.modified_gids.insert(path.clone(), new_gid);
        }

        // Handle mode change (chmod).
        if let Some(new_mode) = mode {
            state
                .modified_modes
                .insert(path.clone(), Mode::from_bits_truncate(new_mode));
        }

        // Handle mtime change (utimensat).
        if let Some(new_mtime) = mtime {
            let time = match new_mtime {
                TimeOrNow::SpecificTime(t) => t,
                TimeOrNow::Now => SystemTime::now(),
            };
            state.set_file_mtime(&path, time);
        }

        // Handle truncation.
        if let Some(new_size) = size {
            // Check size limit to prevent OOM crashes.
            if new_size > MAX_FILE_SIZE {
                reply.error(libc::EFBIG);
                return;
            }

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
                    .map(|(entry_row, _, _)| entry_row.file_size.get())
            })
            .unwrap_or(0);

        let file_mtime = state.get_file_mtime(&path);
        let nlink = state.nlink_counts.get(&ino).copied().unwrap_or(1);

        // Use created_time from archive for ctime/crtime.
        let ctime = state
            .archive
            .find_entry_with_path(&path)
            .map(|(entry_row, _, _)| {
                let ctime_ms = entry_row.created_time.get();
                SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(ctime_ms.max(0) as u64)
            })
            .unwrap_or(file_mtime);

        let perm = state.get_file_mode(&path);

        // Determine correct file type from archive entry (same as getattr).
        let kind = state
            .archive
            .find_entry_with_path(&path)
            .map(|(entry_row, _, _)| match entry_row.kind() {
                EntryKind::Symlink => FileType::Symlink,
                _ => FileType::RegularFile,
            })
            .unwrap_or(FileType::RegularFile);

        let attr = fuser::FileAttr {
            ino,
            size,
            blocks: size.div_ceil(512),
            atime: file_mtime,
            mtime: file_mtime,
            ctime,
            crtime: ctime,
            kind,
            perm: if perm.is_empty() {
                DEFAULT_FILE_PERM.bits() as u16
            } else {
                perm.bits() as u16
            },
            nlink,
            uid: state.get_uid(&path),
            gid: state.get_gid(&path),
            rdev: 0,
            blksize: 4096,
            flags: 0,
        };

        reply.attr(&TTL, &attr);
    }

    /// Creates a new directory.
    fn mkdir(
        &mut self,
        req: &Request<'_>,
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

        // Reject creating a directory named ".bale" (conflicts with virtual entry).
        if name == ".bale" {
            reply.error(libc::EEXIST);
            return;
        }

        let mode = Mode::from_bits_truncate(mode);
        match state.create_directory(parent, name, mode) {
            Ok((ino, mut attr)) => {
                // Set ownership to the calling user.
                let dir_path = state.get_dir_path(ino);
                state.modified_uids.insert(dir_path.clone(), req.uid());
                state.modified_gids.insert(dir_path, req.gid());
                attr.uid = req.uid();
                attr.gid = req.gid();
                reply.entry(&TTL, &attr, ino);
            }
            Err(e) => reply.error(e),
        }
    }

    /// Removes an empty directory.
    fn rmdir(&mut self, req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
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

        // Reject removing the virtual .bale directory.
        if name == ".bale" {
            reply.error(libc::EACCES);
            return;
        }

        match state.remove_directory(parent, name, req.uid()) {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(e),
        }
    }

    /// Creates a new file.
    fn create(
        &mut self,
        req: &Request<'_>,
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

        // Reject creating a file named ".bale" (conflicts with virtual entry).
        if name == ".bale" {
            reply.error(libc::EEXIST);
            return;
        }

        // Reject creating files inside the virtual .bale/ directory.
        if parent == state.virtual_dir.dir_ino() {
            reply.error(libc::EACCES);
            return;
        }

        let mode = Mode::from_bits_truncate(mode);
        match state.create_file(parent, name, mode) {
            Ok((ino, mut attr)) => {
                // Set ownership to the calling user.
                if let Some(path) = state.inode_to_path.get(&ino).cloned() {
                    state.modified_uids.insert(path.clone(), req.uid());
                    state.modified_gids.insert(path, req.gid());
                }
                attr.uid = req.uid();
                attr.gid = req.gid();
                // Use inode as file handle for simplicity.
                reply.created(&TTL, &attr, ino, ino, 0);
            }
            Err(e) => reply.error(e),
        }
    }

    /// Creates a symlink.
    fn symlink(
        &mut self,
        req: &Request<'_>,
        parent: u64,
        link_name: &OsStr,
        target: &std::path::Path,
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

        let link_name = match link_name.to_str() {
            Some(n) => n,
            None => {
                reply.error(libc::EINVAL);
                return;
            }
        };

        // Reject creating a symlink named ".bale" (conflicts with virtual entry).
        if link_name == ".bale" {
            reply.error(libc::EEXIST);
            return;
        }

        let target = match target.to_str() {
            Some(t) => t,
            None => {
                reply.error(libc::EINVAL);
                return;
            }
        };

        match state.create_symlink(parent, link_name, target) {
            Ok((ino, mut attr)) => {
                // Set ownership to the calling user.
                if let Some(path) = state.inode_to_path.get(&ino).cloned() {
                    state.modified_uids.insert(path.clone(), req.uid());
                    state.modified_gids.insert(path, req.gid());
                }
                attr.uid = req.uid();
                attr.gid = req.gid();
                reply.entry(&TTL, &attr, 0);
            }
            Err(e) => reply.error(e),
        }
    }

    /// Creates a hard link.
    fn link(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        newparent: u64,
        newname: &OsStr,
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

        let newname = match newname.to_str() {
            Some(n) => n,
            None => {
                reply.error(libc::EINVAL);
                return;
            }
        };

        // Reject creating a hard link named ".bale" (conflicts with virtual entry).
        if newname == ".bale" {
            reply.error(libc::EEXIST);
            return;
        }

        match state.create_hard_link(ino, newparent, newname) {
            Ok((_ino, attr)) => reply.entry(&TTL, &attr, 0),
            Err(e) => reply.error(e),
        }
    }

    /// Removes a file.
    fn unlink(&mut self, req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
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

        // Reject unlinking the virtual .bale symlink.
        if name == ".bale" {
            reply.error(libc::EACCES);
            return;
        }

        match state.remove_file(parent, name, req.uid()) {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(e),
        }
    }

    /// Renames/moves a file or directory.
    fn rename(
        &mut self,
        req: &Request<'_>,
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

        // Reject renaming .bale entries.
        if old_name == ".bale" || new_name == ".bale" {
            reply.error(libc::EACCES);
            return;
        }

        match state.rename_entry(parent, old_name, newparent, new_name, req.uid()) {
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
            255,                    // namelen (POSIX NAME_MAX)
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
