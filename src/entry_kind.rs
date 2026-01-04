//! Entry type classification based on Unix mode bits.

/// Entry type based on Unix mode bits in external_attrs.
///
/// ZIP archives store Unix file type and permissions in the upper 16 bits
/// of the `external_attrs` field. This enum represents the file type portion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryKind {
    /// Regular file (S_IFREG).
    File,
    /// Directory (S_IFDIR).
    Directory,
    /// Symbolic link (S_IFLNK).
    Symlink,
    /// Unknown or unsupported file type.
    ///
    /// Contains the raw file type bits (masked with S_IFMT).
    Other(u32),
}

impl EntryKind {
    /// Unix file type mask (S_IFMT).
    const S_IFMT: u32 = 0o170000;

    /// Regular file (S_IFREG).
    const S_IFREG: u32 = 0o100000;

    /// Directory (S_IFDIR).
    const S_IFDIR: u32 = 0o040000;

    /// Symbolic link (S_IFLNK).
    const S_IFLNK: u32 = 0o120000;

    /// Determines entry kind from Unix mode bits.
    ///
    /// Extracts the file type from the mode and returns the corresponding
    /// `EntryKind`. Unrecognized types are returned as `Other`.
    #[must_use]
    pub fn from_mode(mode: u32) -> Self {
        match mode & Self::S_IFMT {
            Self::S_IFREG => Self::File,
            Self::S_IFDIR => Self::Directory,
            Self::S_IFLNK => Self::Symlink,
            // Mode 0 (no type bits) defaults to File for compatibility.
            // Many ZIP tools don't set type bits for regular files.
            0 => Self::File,
            other => Self::Other(other),
        }
    }

    /// Returns true if this is a regular file.
    #[must_use]
    pub fn is_file(&self) -> bool {
        matches!(self, Self::File)
    }

    /// Returns true if this is a directory.
    #[must_use]
    pub fn is_directory(&self) -> bool {
        matches!(self, Self::Directory)
    }

    /// Returns true if this is a symbolic link.
    #[must_use]
    pub fn is_symlink(&self) -> bool {
        matches!(self, Self::Symlink)
    }
}

impl std::fmt::Display for EntryKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File => write!(f, "file"),
            Self::Directory => write!(f, "directory"),
            Self::Symlink => write!(f, "symlink"),
            Self::Other(mode) => write!(f, "unknown({mode:#o})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regular file mode bits are recognized.
    #[test]
    fn file_mode() {
        assert_eq!(EntryKind::from_mode(0o100644), EntryKind::File);
        assert_eq!(EntryKind::from_mode(0o100755), EntryKind::File);
    }

    /// Directory mode bits are recognized.
    #[test]
    fn directory_mode() {
        assert_eq!(EntryKind::from_mode(0o040755), EntryKind::Directory);
        assert_eq!(EntryKind::from_mode(0o040000), EntryKind::Directory);
    }

    /// Symlink mode bits are recognized.
    #[test]
    fn symlink_mode() {
        assert_eq!(EntryKind::from_mode(0o120777), EntryKind::Symlink);
        assert_eq!(EntryKind::from_mode(0o120000), EntryKind::Symlink);
    }

    /// Zero mode defaults to file for compatibility.
    #[test]
    fn zero_mode_defaults_to_file() {
        assert_eq!(EntryKind::from_mode(0), EntryKind::File);
    }

    /// Unknown mode bits are captured in Other.
    #[test]
    fn unknown_mode() {
        // Block device (S_IFBLK = 0o060000)
        let kind = EntryKind::from_mode(0o060644);
        assert_eq!(kind, EntryKind::Other(0o060000));
    }

    /// is_* predicates work correctly.
    #[test]
    fn predicates() {
        assert!(EntryKind::File.is_file());
        assert!(!EntryKind::File.is_directory());
        assert!(!EntryKind::File.is_symlink());

        assert!(!EntryKind::Directory.is_file());
        assert!(EntryKind::Directory.is_directory());
        assert!(!EntryKind::Directory.is_symlink());

        assert!(!EntryKind::Symlink.is_file());
        assert!(!EntryKind::Symlink.is_directory());
        assert!(EntryKind::Symlink.is_symlink());
    }

    /// Display formatting.
    #[test]
    fn display() {
        assert_eq!(format!("{}", EntryKind::File), "file");
        assert_eq!(format!("{}", EntryKind::Directory), "directory");
        assert_eq!(format!("{}", EntryKind::Symlink), "symlink");
        assert_eq!(
            format!("{}", EntryKind::Other(0o060000)),
            "unknown(0o60000)"
        );
    }
}
