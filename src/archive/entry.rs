//! Generic entry wrapper for any archive entry type.

use crate::archive::{DirEntry, FileEntry, SymlinkEntry};

/// An entry in the archive of any type.
///
/// This enum provides generic access to archive entries when the caller
/// needs to handle any entry type. For type-specific access, use
/// [`ArchiveRead::file`], [`ArchiveRead::folder`], or the entry's
/// conversion methods.
///
/// # Example
///
/// ```ignore
/// match archive.entry("some/path")? {
///     Entry::File(f) => println!("file: {} bytes", f.size()),
///     Entry::Directory(d) => println!("dir: {}", d.path()),
///     Entry::Symlink(s) => println!("link -> {}", s.target().unwrap_or("<binary>")),
/// }
/// ```
#[derive(Debug)]
pub enum Entry<'a> {
    /// A regular file entry.
    File(FileEntry<'a>),
    /// A directory entry.
    Directory(DirEntry<'a>),
    /// A symbolic link entry.
    Symlink(SymlinkEntry<'a>),
}

impl<'a> Entry<'a> {
    /// Returns `true` if this is a file entry.
    #[must_use]
    pub fn is_file(&self) -> bool {
        matches!(self, Self::File(_))
    }

    /// Returns `true` if this is a directory entry.
    #[must_use]
    pub fn is_directory(&self) -> bool {
        matches!(self, Self::Directory(_))
    }

    /// Returns `true` if this is a symlink entry.
    #[must_use]
    pub fn is_symlink(&self) -> bool {
        matches!(self, Self::Symlink(_))
    }

    /// Returns the file entry if this is a file, or `None` otherwise.
    #[must_use]
    pub fn as_file(&self) -> Option<&FileEntry<'a>> {
        match self {
            Self::File(f) => Some(f),
            _ => None,
        }
    }

    /// Returns the directory entry if this is a directory, or `None` otherwise.
    #[must_use]
    pub fn as_directory(&self) -> Option<&DirEntry<'a>> {
        match self {
            Self::Directory(d) => Some(d),
            _ => None,
        }
    }

    /// Returns the symlink entry if this is a symlink, or `None` otherwise.
    #[must_use]
    pub fn as_symlink(&self) -> Option<&SymlinkEntry<'a>> {
        match self {
            Self::Symlink(s) => Some(s),
            _ => None,
        }
    }

    /// Converts into a file entry if this is a file, or `None` otherwise.
    #[must_use]
    pub fn into_file(self) -> Option<FileEntry<'a>> {
        match self {
            Self::File(f) => Some(f),
            _ => None,
        }
    }

    /// Converts into a directory entry if this is a directory, or `None` otherwise.
    #[must_use]
    pub fn into_directory(self) -> Option<DirEntry<'a>> {
        match self {
            Self::Directory(d) => Some(d),
            _ => None,
        }
    }

    /// Converts into a symlink entry if this is a symlink, or `None` otherwise.
    #[must_use]
    pub fn into_symlink(self) -> Option<SymlinkEntry<'a>> {
        match self {
            Self::Symlink(s) => Some(s),
            _ => None,
        }
    }
}

impl<'a> From<FileEntry<'a>> for Entry<'a> {
    fn from(entry: FileEntry<'a>) -> Self {
        Self::File(entry)
    }
}

impl<'a> From<DirEntry<'a>> for Entry<'a> {
    fn from(entry: DirEntry<'a>) -> Self {
        Self::Directory(entry)
    }
}

impl<'a> From<SymlinkEntry<'a>> for Entry<'a> {
    fn from(entry: SymlinkEntry<'a>) -> Self {
        Self::Symlink(entry)
    }
}
