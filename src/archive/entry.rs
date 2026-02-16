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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ArchivePath;
    use crate::format::{Crc, EntryRow};

    /// Creates a test `FileEntry`.
    fn make_file_entry(row: &EntryRow) -> FileEntry<'_> {
        FileEntry {
            entry: row,
            path: ArchivePath::from_bytes(b"test.txt"),
            data: b"hello",
            id: 1,
        }
    }

    /// Creates a test `DirEntry`.
    fn make_dir_entry(row: &EntryRow) -> DirEntry<'_> {
        DirEntry {
            entry: row,
            path: ArchivePath::from_bytes(b"src"),
            id: 2,
        }
    }

    /// Creates a test `SymlinkEntry`.
    fn make_symlink_entry(row: &EntryRow) -> SymlinkEntry<'_> {
        SymlinkEntry {
            entry: row,
            path: ArchivePath::from_bytes(b"link"),
            target: b"target",
            id: 3,
        }
    }

    // ==================== is_* predicates ====================

    /// `is_file()` returns true only for `Entry::File`.
    #[test]
    fn is_file_predicate() {
        let file_row = EntryRow::new_file(1, Crc::NONE, 0, 0, 0, 0, 0, 0o100644);
        let dir_row = EntryRow::new_directory(2, 0, 0, 0o040755);
        let sym_row = EntryRow::new_file(3, Crc::NONE, 0, 0, 0, 0, 0, 0o120777);

        let file = Entry::File(make_file_entry(&file_row));
        let dir = Entry::Directory(make_dir_entry(&dir_row));
        let sym = Entry::Symlink(make_symlink_entry(&sym_row));

        assert!(file.is_file());
        assert!(!dir.is_file());
        assert!(!sym.is_file());
    }

    /// `is_directory()` returns true only for `Entry::Directory`.
    #[test]
    fn is_directory_predicate() {
        let file_row = EntryRow::new_file(1, Crc::NONE, 0, 0, 0, 0, 0, 0o100644);
        let dir_row = EntryRow::new_directory(2, 0, 0, 0o040755);
        let sym_row = EntryRow::new_file(3, Crc::NONE, 0, 0, 0, 0, 0, 0o120777);

        let file = Entry::File(make_file_entry(&file_row));
        let dir = Entry::Directory(make_dir_entry(&dir_row));
        let sym = Entry::Symlink(make_symlink_entry(&sym_row));

        assert!(!file.is_directory());
        assert!(dir.is_directory());
        assert!(!sym.is_directory());
    }

    /// `is_symlink()` returns true only for `Entry::Symlink`.
    #[test]
    fn is_symlink_predicate() {
        let file_row = EntryRow::new_file(1, Crc::NONE, 0, 0, 0, 0, 0, 0o100644);
        let dir_row = EntryRow::new_directory(2, 0, 0, 0o040755);
        let sym_row = EntryRow::new_file(3, Crc::NONE, 0, 0, 0, 0, 0, 0o120777);

        let file = Entry::File(make_file_entry(&file_row));
        let dir = Entry::Directory(make_dir_entry(&dir_row));
        let sym = Entry::Symlink(make_symlink_entry(&sym_row));

        assert!(!file.is_symlink());
        assert!(!dir.is_symlink());
        assert!(sym.is_symlink());
    }

    // ==================== as_* borrow conversions ====================

    /// `as_file()` returns `Some` for file entries and `None` for others.
    #[test]
    fn as_file_conversion() {
        let file_row = EntryRow::new_file(1, Crc::NONE, 0, 0, 0, 0, 0, 0o100644);
        let dir_row = EntryRow::new_directory(2, 0, 0, 0o040755);
        let sym_row = EntryRow::new_file(3, Crc::NONE, 0, 0, 0, 0, 0, 0o120777);

        let file = Entry::File(make_file_entry(&file_row));
        let dir = Entry::Directory(make_dir_entry(&dir_row));
        let sym = Entry::Symlink(make_symlink_entry(&sym_row));

        assert!(file.as_file().is_some());
        assert_eq!(file.as_file().unwrap().id(), 1);
        assert!(dir.as_file().is_none());
        assert!(sym.as_file().is_none());
    }

    /// `as_directory()` returns `Some` for directory entries and `None` for others.
    #[test]
    fn as_directory_conversion() {
        let file_row = EntryRow::new_file(1, Crc::NONE, 0, 0, 0, 0, 0, 0o100644);
        let dir_row = EntryRow::new_directory(2, 0, 0, 0o040755);
        let sym_row = EntryRow::new_file(3, Crc::NONE, 0, 0, 0, 0, 0, 0o120777);

        let file = Entry::File(make_file_entry(&file_row));
        let dir = Entry::Directory(make_dir_entry(&dir_row));
        let sym = Entry::Symlink(make_symlink_entry(&sym_row));

        assert!(file.as_directory().is_none());
        assert!(dir.as_directory().is_some());
        assert_eq!(dir.as_directory().unwrap().id(), 2);
        assert!(sym.as_directory().is_none());
    }

    /// `as_symlink()` returns `Some` for symlink entries and `None` for others.
    #[test]
    fn as_symlink_conversion() {
        let file_row = EntryRow::new_file(1, Crc::NONE, 0, 0, 0, 0, 0, 0o100644);
        let dir_row = EntryRow::new_directory(2, 0, 0, 0o040755);
        let sym_row = EntryRow::new_file(3, Crc::NONE, 0, 0, 0, 0, 0, 0o120777);

        let file = Entry::File(make_file_entry(&file_row));
        let dir = Entry::Directory(make_dir_entry(&dir_row));
        let sym = Entry::Symlink(make_symlink_entry(&sym_row));

        assert!(file.as_symlink().is_none());
        assert!(dir.as_symlink().is_none());
        assert!(sym.as_symlink().is_some());
        assert_eq!(sym.as_symlink().unwrap().id(), 3);
    }

    // ==================== into_* owned conversions ====================

    /// `into_file()` returns `Some` for file entries and `None` for others.
    #[test]
    fn into_file_conversion() {
        let file_row = EntryRow::new_file(1, Crc::NONE, 0, 0, 0, 0, 0, 0o100644);
        let dir_row = EntryRow::new_directory(2, 0, 0, 0o040755);

        let file = Entry::File(make_file_entry(&file_row));
        assert_eq!(file.into_file().unwrap().id(), 1);

        let dir = Entry::Directory(make_dir_entry(&dir_row));
        assert!(dir.into_file().is_none());
    }

    /// `into_directory()` returns `Some` for directory entries and `None` for others.
    #[test]
    fn into_directory_conversion() {
        let dir_row = EntryRow::new_directory(2, 0, 0, 0o040755);
        let sym_row = EntryRow::new_file(3, Crc::NONE, 0, 0, 0, 0, 0, 0o120777);

        let dir = Entry::Directory(make_dir_entry(&dir_row));
        assert_eq!(dir.into_directory().unwrap().id(), 2);

        let sym = Entry::Symlink(make_symlink_entry(&sym_row));
        assert!(sym.into_directory().is_none());
    }

    /// `into_symlink()` returns `Some` for symlink entries and `None` for others.
    #[test]
    fn into_symlink_conversion() {
        let file_row = EntryRow::new_file(1, Crc::NONE, 0, 0, 0, 0, 0, 0o100644);
        let sym_row = EntryRow::new_file(3, Crc::NONE, 0, 0, 0, 0, 0, 0o120777);

        let sym = Entry::Symlink(make_symlink_entry(&sym_row));
        assert_eq!(sym.into_symlink().unwrap().id(), 3);

        let file = Entry::File(make_file_entry(&file_row));
        assert!(file.into_symlink().is_none());
    }

    // ==================== From impls ====================

    /// `From<FileEntry>` produces `Entry::File`.
    #[test]
    fn from_file_entry() {
        let row = EntryRow::new_file(1, Crc::NONE, 0, 0, 0, 0, 0, 0o100644);
        let entry: Entry<'_> = make_file_entry(&row).into();
        assert!(entry.is_file());
    }

    /// `From<DirEntry>` produces `Entry::Directory`.
    #[test]
    fn from_dir_entry() {
        let row = EntryRow::new_directory(2, 0, 0, 0o040755);
        let entry: Entry<'_> = make_dir_entry(&row).into();
        assert!(entry.is_directory());
    }

    /// `From<SymlinkEntry>` produces `Entry::Symlink`.
    #[test]
    fn from_symlink_entry() {
        let row = EntryRow::new_file(3, Crc::NONE, 0, 0, 0, 0, 0, 0o120777);
        let entry: Entry<'_> = make_symlink_entry(&row).into();
        assert!(entry.is_symlink());
    }

    // ==================== Property Tests ====================

    use proptest::prelude::*;

    use crate::proptest_config;

    proptest! {
        #![proptest_config(proptest_config::config())]

        /// `Entry::File` satisfies all file predicates and conversions.
        #[test]
        fn file_variant_properties(
            id in 1..=u32::MAX,
            mode in any::<u32>(),
        ) {
            let row = EntryRow::new_file(id, Crc::NONE, 0, 0, 0, 0, 0, mode);
            let entry = Entry::File(FileEntry {
                entry: &row,
                path: ArchivePath::from_bytes(b"f"),
                data: b"",
                id,
            });
            prop_assert!(entry.is_file());
            prop_assert!(!entry.is_directory());
            prop_assert!(!entry.is_symlink());
            prop_assert!(entry.as_file().is_some());
            prop_assert_eq!(entry.as_file().unwrap().id(), id);
            prop_assert!(entry.as_directory().is_none());
            prop_assert!(entry.as_symlink().is_none());
        }

        /// `Entry::Directory` satisfies all directory predicates and conversions.
        #[test]
        fn directory_variant_properties(
            id in 1..=u32::MAX,
            mode in any::<u32>(),
        ) {
            let row = EntryRow::new_directory(id, 0, 0, mode);
            let entry = Entry::Directory(DirEntry {
                entry: &row,
                path: ArchivePath::from_bytes(b"d"),
                id,
            });
            prop_assert!(!entry.is_file());
            prop_assert!(entry.is_directory());
            prop_assert!(!entry.is_symlink());
            prop_assert!(entry.as_file().is_none());
            prop_assert!(entry.as_directory().is_some());
            prop_assert_eq!(entry.as_directory().unwrap().id(), id);
            prop_assert!(entry.as_symlink().is_none());
        }

        /// `Entry::Symlink` satisfies all symlink predicates and conversions.
        #[test]
        fn symlink_variant_properties(
            id in 1..=u32::MAX,
            mode in any::<u32>(),
        ) {
            let row = EntryRow::new_file(id, Crc::NONE, 0, 0, 0, 0, 0, mode);
            let entry = Entry::Symlink(SymlinkEntry {
                entry: &row,
                path: ArchivePath::from_bytes(b"s"),
                target: b"t",
                id,
            });
            prop_assert!(!entry.is_file());
            prop_assert!(!entry.is_directory());
            prop_assert!(entry.is_symlink());
            prop_assert!(entry.as_file().is_none());
            prop_assert!(entry.as_directory().is_none());
            prop_assert!(entry.as_symlink().is_some());
            prop_assert_eq!(entry.as_symlink().unwrap().id(), id);
        }

        /// `From` conversion preserves the entry ID through `into_*`.
        #[test]
        fn from_into_round_trip(id in 1..=u32::MAX) {
            let file_row = EntryRow::new_file(id, Crc::NONE, 0, 0, 0, 0, 0, 0o100644);
            let dir_row = EntryRow::new_directory(id, 0, 0, 0o040755);
            let sym_row = EntryRow::new_file(id, Crc::NONE, 0, 0, 0, 0, 0, 0o120777);

            let file_entry = FileEntry {
                entry: &file_row, path: ArchivePath::from_bytes(b"f"),
                data: b"", id,
            };
            let dir_entry_val = DirEntry {
                entry: &dir_row, path: ArchivePath::from_bytes(b"d"), id,
            };
            let sym_entry = SymlinkEntry {
                entry: &sym_row, path: ArchivePath::from_bytes(b"s"),
                target: b"t", id,
            };

            let e: Entry<'_> = file_entry.into();
            prop_assert_eq!(e.into_file().unwrap().id(), id);

            let e: Entry<'_> = dir_entry_val.into();
            prop_assert_eq!(e.into_directory().unwrap().id(), id);

            let e: Entry<'_> = sym_entry.into();
            prop_assert_eq!(e.into_symlink().unwrap().id(), id);
        }
    }
}
