//! Directory entry for FUSE filesystem.

use fuser::FileType;

/// A directory entry for FUSE readdir operations.
///
/// This struct represents a single entry in a directory listing,
/// containing the entry name, inode number, and file type.
#[derive(Debug, Clone)]
pub struct FuseDirEntry {
    /// The entry name (file or directory name, not full path).
    pub name: String,
    /// The inode number for this entry.
    pub ino: u64,
    /// The file type (regular file, directory, symlink, etc.).
    pub kind: FileType,
}

impl FuseDirEntry {
    /// Creates a new directory entry.
    #[must_use]
    pub fn new(name: impl Into<String>, ino: u64, kind: FileType) -> Self {
        Self {
            name: name.into(),
            ino,
            kind,
        }
    }

    /// Creates a directory entry for a regular file.
    #[must_use]
    pub fn file(name: impl Into<String>, ino: u64) -> Self {
        Self::new(name, ino, FileType::RegularFile)
    }

    /// Creates a directory entry for a directory.
    #[must_use]
    pub fn directory(name: impl Into<String>, ino: u64) -> Self {
        Self::new(name, ino, FileType::Directory)
    }

    /// Creates a directory entry for a symbolic link.
    #[must_use]
    pub fn symlink(name: impl Into<String>, ino: u64) -> Self {
        Self::new(name, ino, FileType::Symlink)
    }
}
