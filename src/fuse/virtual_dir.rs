//! Virtual `.bale/` metadata directory.
//!
//! Provides a read-only `.bale/` directory at the mount root containing
//! archive metadata files, plus hidden `.bale` symlinks in every
//! subdirectory so users can detect they're inside a bale mount from
//! any directory.

use std::collections::HashMap;

use fuser::FileType;

use crate::fuse::inode::VIRTUAL_INO_START;
use crate::{ArchiveRead, ArchiveWriter};

/// Number of metadata files inside the `.bale/` directory.
const VIRTUAL_FILE_COUNT: u64 = 7;

/// A single metadata file inside the virtual `.bale/` directory.
struct VirtualFile {
    /// Filename (e.g. `"version"`).
    name: &'static str,
    /// Assigned inode number.
    ino: u64,
}

/// Virtual `.bale/` directory and its contents.
///
/// Manages inode allocation for the virtual directory, its metadata
/// files, and per-directory `.bale` symlink entries so that each
/// subdirectory gets a unique inode with the correct relative target.
pub(super) struct VirtualDir {
    /// Inode of the `.bale/` directory itself.
    dir_ino: u64,
    /// Next available virtual inode for symlink allocation.
    next_symlink_ino: u64,
    /// Metadata files inside `.bale/`.
    files: Vec<VirtualFile>,
    /// Per-directory symlink inodes: maps directory inode → symlink inode.
    symlink_inodes: HashMap<u64, u64>,
    /// Reverse map: symlink inode → directory depth (for `readlink`).
    symlink_depths: HashMap<u64, usize>,
}

impl VirtualDir {
    /// Creates a new `VirtualDir` with pre-allocated inodes.
    ///
    /// Inode layout starting from `VIRTUAL_INO_START`:
    /// - `+0`: `.bale/` directory
    /// - `+1` through `+N`: metadata files
    /// - `+N+1...`: per-directory symlink inodes (allocated on demand)
    pub(super) fn new() -> Self {
        let dir_ino = VIRTUAL_INO_START;

        let file_names = [
            "version",
            "entry_count",
            "directory_count",
            "path_size",
            "alignment",
            "archive_size",
            "compacted",
        ];

        let files: Vec<VirtualFile> = file_names
            .iter()
            .enumerate()
            .map(|(i, &name)| VirtualFile {
                name,
                ino: VIRTUAL_INO_START + 1 + i as u64,
            })
            .collect();

        Self {
            dir_ino,
            next_symlink_ino: VIRTUAL_INO_START + 1 + VIRTUAL_FILE_COUNT,
            files,
            symlink_inodes: HashMap::new(),
            symlink_depths: HashMap::new(),
        }
    }

    /// Returns the inode of the `.bale/` directory.
    pub(super) fn dir_ino(&self) -> u64 {
        self.dir_ino
    }

    /// Returns (or allocates) the symlink inode for a directory at the
    /// given depth.
    ///
    /// Each directory gets a unique symlink inode so that `readlink` can
    /// return the correct relative target.
    pub(super) fn symlink_ino_for(&mut self, dir_ino: u64, depth: usize) -> u64 {
        if let Some(&ino) = self.symlink_inodes.get(&dir_ino) {
            return ino;
        }
        let ino = self.next_symlink_ino;
        self.next_symlink_ino += 1;
        self.symlink_inodes.insert(dir_ino, ino);
        self.symlink_depths.insert(ino, depth);
        ino
    }

    /// Returns the directory depth for a symlink inode, if it is a
    /// virtual symlink.
    pub(super) fn symlink_depth(&self, ino: u64) -> Option<usize> {
        self.symlink_depths.get(&ino).copied()
    }

    /// Looks up an entry inside the `.bale/` directory by name.
    ///
    /// Returns the inode and file type if found.
    pub(super) fn lookup(&self, name: &str) -> Option<(u64, FileType)> {
        for file in &self.files {
            if file.name == name {
                return Some((file.ino, FileType::RegularFile));
            }
        }
        None
    }

    /// Iterates over all entries in the `.bale/` directory.
    ///
    /// Yields `(inode, file_type, name)` tuples suitable for readdir.
    pub(super) fn readdir(&self) -> impl Iterator<Item = (u64, FileType, &str)> {
        self.files
            .iter()
            .map(|f| (f.ino, FileType::RegularFile, f.name))
    }

    /// Generates the text content for a virtual metadata file.
    ///
    /// Returns `None` if the inode does not belong to a virtual file.
    pub(super) fn file_content(&self, ino: u64, archive: &ArchiveWriter) -> Option<String> {
        let file = self.files.iter().find(|f| f.ino == ino)?;
        let trailer = archive.trailer();

        let content = match file.name {
            "version" => {
                let header = crate::format::FileHeader::new();
                let (major, minor, patch) = header.version();
                format!("{major}.{minor}.{patch}\n")
            }
            "entry_count" => format!("{}\n", trailer.entry_count.get()),
            "directory_count" => format!("{}\n", trailer.directory_entry_count.get()),
            "path_size" => format!("{}\n", trailer.path_size()),
            "alignment" => {
                let alignment = trailer.alignment().unwrap_or(4096);
                format!("{alignment}\n")
            }
            "archive_size" => format!("{}\n", trailer.archive_size.get()),
            "compacted" => {
                let val = if trailer.is_compacted() {
                    "true"
                } else {
                    "false"
                };
                format!("{val}\n")
            }
            _ => return None,
        };
        Some(content)
    }

    /// Returns `true` if the given inode belongs to the virtual directory,
    /// one of its files, or any per-directory symlink.
    pub(super) fn is_virtual(&self, ino: u64) -> bool {
        if ino == self.dir_ino {
            return true;
        }
        if self.symlink_depths.contains_key(&ino) {
            return true;
        }
        self.files.iter().any(|f| f.ino == ino)
    }

    /// Returns the relative symlink target for a `.bale` symlink at the
    /// given directory depth.
    ///
    /// - depth 0 (root): `.bale` (direct reference)
    /// - depth 1: `../.bale`
    /// - depth 2: `../../.bale`
    pub(super) fn symlink_target(depth: usize) -> String {
        if depth == 0 {
            ".bale".to_string()
        } else {
            let prefix: String = (0..depth).map(|_| "..").collect::<Vec<_>>().join("/");
            format!("{prefix}/.bale")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    /// Creates a test archive with the given entries.
    fn create_test_archive(entries: &[(&str, &[u8], u32)]) -> ArchiveWriter {
        use crate::ArchiveWrite;

        let dir = tempdir().unwrap();
        let path = dir.path().join("test.bale");

        let mut writer = ArchiveWriter::create(&path).unwrap();
        for (name, data, mode) in entries {
            writer.add_entry(name, data, *mode).unwrap();
        }
        writer.sync().unwrap();
        drop(writer);

        let writer = ArchiveWriter::open(&path).unwrap();
        std::mem::forget(dir);
        writer
    }

    /// `new()` allocates correct inodes.
    #[test]
    fn new_allocates_correct_inodes() {
        let vdir = VirtualDir::new();
        assert_eq!(vdir.dir_ino(), VIRTUAL_INO_START);
        // All file inodes should start after dir_ino.
        for (i, file) in vdir.files.iter().enumerate() {
            assert_eq!(file.ino, VIRTUAL_INO_START + 1 + i as u64);
        }
    }

    /// `symlink_ino_for()` allocates unique inodes per directory.
    #[test]
    fn symlink_ino_for_allocates_unique_inodes() {
        let mut vdir = VirtualDir::new();
        let ino_a = vdir.symlink_ino_for(100, 1);
        let ino_b = vdir.symlink_ino_for(200, 2);
        assert_ne!(ino_a, ino_b, "different dirs get different inodes");

        // Same dir returns same inode.
        let ino_a2 = vdir.symlink_ino_for(100, 1);
        assert_eq!(ino_a, ino_a2, "same dir returns same inode");
    }

    /// `symlink_depth()` returns the correct depth for allocated symlinks.
    #[test]
    fn symlink_depth_returns_correct_values() {
        let mut vdir = VirtualDir::new();
        let ino = vdir.symlink_ino_for(100, 3);
        assert_eq!(vdir.symlink_depth(ino), Some(3));
        assert_eq!(vdir.symlink_depth(999), None);
    }

    /// `is_virtual()` returns true for virtual inodes.
    #[test]
    fn is_virtual_true_for_virtual_inodes() {
        let mut vdir = VirtualDir::new();
        assert!(vdir.is_virtual(vdir.dir_ino()));
        // Allocated symlink inodes are virtual.
        let sym_ino = vdir.symlink_ino_for(100, 1);
        assert!(vdir.is_virtual(sym_ino));
        // Check all file inodes.
        for file in &vdir.files {
            assert!(vdir.is_virtual(file.ino));
        }
    }

    /// `is_virtual()` returns false for non-virtual inodes.
    #[test]
    fn is_virtual_false_for_non_virtual_inodes() {
        let vdir = VirtualDir::new();
        assert!(!vdir.is_virtual(1));
        assert!(!vdir.is_virtual(0x1_0000_0000));
        assert!(!vdir.is_virtual(0x2_0000_0000));
        assert!(!vdir.is_virtual(0x4_0000_0000));
    }

    /// `lookup("version")` returns the correct inode and `RegularFile` type.
    #[test]
    fn lookup_version_returns_correct_inode() {
        let vdir = VirtualDir::new();
        let result = vdir.lookup("version");
        assert!(result.is_some());
        let (ino, kind) = result.unwrap();
        assert_eq!(ino, VIRTUAL_INO_START + 1);
        assert_eq!(kind, FileType::RegularFile);
    }

    /// `lookup("nonexistent")` returns `None`.
    #[test]
    fn lookup_nonexistent_returns_none() {
        let vdir = VirtualDir::new();
        assert!(vdir.lookup("nonexistent").is_none());
    }

    /// `readdir()` yields all expected files.
    #[test]
    fn readdir_yields_all_files() {
        let vdir = VirtualDir::new();
        let entries: Vec<_> = vdir.readdir().collect();
        assert_eq!(entries.len(), 7);

        let names: Vec<&str> = entries.iter().map(|(_, _, n)| *n).collect();
        assert!(names.contains(&"version"));
        assert!(names.contains(&"entry_count"));
        assert!(names.contains(&"directory_count"));
        assert!(names.contains(&"path_size"));
        assert!(names.contains(&"alignment"));
        assert!(names.contains(&"archive_size"));
        assert!(names.contains(&"compacted"));

        // All should be RegularFile.
        for (_, kind, _) in &entries {
            assert_eq!(*kind, FileType::RegularFile);
        }
    }

    /// `file_content()` returns correct formatted strings for each file.
    #[test]
    fn file_content_returns_correct_values() {
        let archive = create_test_archive(&[
            ("file1.txt", b"hello", 0o100644),
            ("file2.txt", b"world", 0o100644),
        ]);

        let vdir = VirtualDir::new();

        // Check version.
        let version_ino = vdir.lookup("version").unwrap().0;
        let content = vdir.file_content(version_ino, &archive).unwrap();
        assert_eq!(content, "1.0.0\n");

        // Check entry_count.
        let ec_ino = vdir.lookup("entry_count").unwrap().0;
        let content = vdir.file_content(ec_ino, &archive).unwrap();
        assert_eq!(content, "2\n");

        // Check path_size (default 256).
        let ps_ino = vdir.lookup("path_size").unwrap().0;
        let content = vdir.file_content(ps_ino, &archive).unwrap();
        assert_eq!(content, "256\n");

        // Check alignment (default 4096).
        let al_ino = vdir.lookup("alignment").unwrap().0;
        let content = vdir.file_content(al_ino, &archive).unwrap();
        assert_eq!(content, "4096\n");

        // Check compacted.
        let cp_ino = vdir.lookup("compacted").unwrap().0;
        let content = vdir.file_content(cp_ino, &archive).unwrap();
        // Newly created archives have the compacted flag set after sync.
        assert!(content == "true\n" || content == "false\n");
    }

    /// `file_content()` returns `None` for non-virtual inodes.
    #[test]
    fn file_content_returns_none_for_non_virtual() {
        let archive = create_test_archive(&[]);
        let mut vdir = VirtualDir::new();
        let sym_ino = vdir.symlink_ino_for(100, 1);
        assert!(vdir.file_content(1, &archive).is_none());
        assert!(vdir.file_content(vdir.dir_ino(), &archive).is_none());
        assert!(vdir.file_content(sym_ino, &archive).is_none());
    }

    /// `symlink_target(0)` → `.bale`.
    #[test]
    fn symlink_target_depth_0() {
        assert_eq!(VirtualDir::symlink_target(0), ".bale");
    }

    /// `symlink_target(1)` → `../.bale`.
    #[test]
    fn symlink_target_depth_1() {
        assert_eq!(VirtualDir::symlink_target(1), "../.bale");
    }

    /// `symlink_target(3)` → `../../../.bale`.
    #[test]
    fn symlink_target_depth_3() {
        assert_eq!(VirtualDir::symlink_target(3), "../../../.bale");
    }
}
