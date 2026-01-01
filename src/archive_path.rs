//! Archive path type for validated, normalized paths within a bale archive.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};

use crate::BaleError;

/// A normalized path within a bale archive.
///
/// Archive paths use forward slashes as separators and are typically UTF-8,
/// but may contain arbitrary bytes when read from an archive.
///
/// # Construction
///
/// - Use `TryFrom<&str>`, `TryFrom<&Path>`, etc. to create from user input
///   (validates UTF-8 and normalizes)
/// - Use `From<Vec<u8>>` or `From<&[u8]>` to create from raw archive bytes
///   (no validation, used when reading archives)
///
/// # Display
///
/// The `Display` implementation uses lossy UTF-8 conversion, replacing invalid
/// bytes with the Unicode replacement character.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArchivePath(Vec<u8>);

impl ArchivePath {
    /// Returns the path as a string slice if it is valid UTF-8.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.0).ok()
    }

    /// Returns the path as bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Returns the length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns true if the path is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Normalizes a path string for archive storage.
    ///
    /// - Trims leading/trailing whitespace
    /// - Converts backslashes to forward slashes
    /// - Removes `.` components
    /// - Resolves `..` components
    /// - Removes leading/trailing slashes
    /// - Collapses multiple slashes
    fn normalize(path: &str) -> Vec<u8> {
        let mut components: Vec<&str> = Vec::new();

        for part in path.trim().split(['/', '\\']) {
            match part {
                "" | "." => {}
                ".." => {
                    components.pop();
                }
                component => {
                    components.push(component);
                }
            }
        }

        components.join("/").into_bytes()
    }
}

/// Displays the path, using lossy UTF-8 conversion for invalid bytes.
impl fmt::Display for ArchivePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", String::from_utf8_lossy(&self.0))
    }
}

/// Allows `ArchivePath` to be used where a `&[u8]` is expected.
impl AsRef<[u8]> for ArchivePath {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// Creates an `ArchivePath` from raw bytes without validation.
///
/// Use this when reading paths from an existing archive.
impl From<Vec<u8>> for ArchivePath {
    fn from(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

/// Creates an `ArchivePath` from raw bytes without validation.
///
/// Use this when reading paths from an existing archive.
impl From<&[u8]> for ArchivePath {
    fn from(bytes: &[u8]) -> Self {
        Self(bytes.to_vec())
    }
}

/// Consumes the `ArchivePath` and returns the inner bytes.
impl From<ArchivePath> for Vec<u8> {
    fn from(path: ArchivePath) -> Self {
        path.0
    }
}

/// Converts a `&str` to an `ArchivePath` with normalization.
impl TryFrom<&str> for ArchivePath {
    type Error = BaleError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Ok(Self(Self::normalize(s)))
    }
}

/// Converts a `String` to an `ArchivePath` with normalization.
impl TryFrom<String> for ArchivePath {
    type Error = BaleError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Ok(Self(Self::normalize(&s)))
    }
}

/// Converts a `&Path` to an `ArchivePath` with normalization.
///
/// # Errors
///
/// Returns `BaleError::InvalidPath` if the path is not valid UTF-8.
impl TryFrom<&Path> for ArchivePath {
    type Error = BaleError;

    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        let s = path.to_str().ok_or(BaleError::InvalidPath)?;
        Ok(Self(Self::normalize(s)))
    }
}

/// Converts a `PathBuf` to an `ArchivePath` with normalization.
///
/// # Errors
///
/// Returns `BaleError::InvalidPath` if the path is not valid UTF-8.
impl TryFrom<PathBuf> for ArchivePath {
    type Error = BaleError;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        Self::try_from(path.as_path())
    }
}

/// Converts an `&OsStr` to an `ArchivePath` with normalization.
///
/// # Errors
///
/// Returns `BaleError::InvalidPath` if the string is not valid UTF-8.
impl TryFrom<&OsStr> for ArchivePath {
    type Error = BaleError;

    fn try_from(s: &OsStr) -> Result<Self, Self::Error> {
        Self::try_from(Path::new(s))
    }
}

/// Converts an `OsString` to an `ArchivePath` with normalization.
///
/// # Errors
///
/// Returns `BaleError::InvalidPath` if the string is not valid UTF-8.
impl TryFrom<OsString> for ArchivePath {
    type Error = BaleError;

    fn try_from(s: OsString) -> Result<Self, Self::Error> {
        Self::try_from(s.as_os_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Forward slashes are preserved.
    #[test]
    fn forward_slashes_preserved() {
        let path = ArchivePath::try_from("foo/bar/baz").unwrap();
        assert_eq!(path.as_str(), Some("foo/bar/baz"));
    }

    /// Backslashes are converted to forward slashes.
    #[test]
    fn backslashes_converted() {
        let path = ArchivePath::try_from(Path::new("foo\\bar\\baz")).unwrap();
        assert_eq!(path.as_str(), Some("foo/bar/baz"));
    }

    /// Leading slashes are stripped.
    #[test]
    fn leading_slashes_stripped() {
        let path = ArchivePath::try_from(Path::new("/foo/bar")).unwrap();
        assert_eq!(path.as_str(), Some("foo/bar"));
    }

    /// Multiple slashes are collapsed.
    #[test]
    fn multiple_slashes_collapsed() {
        let path = ArchivePath::try_from(Path::new("foo//bar///baz")).unwrap();
        assert_eq!(path.as_str(), Some("foo/bar/baz"));
    }

    /// Trailing slashes are stripped.
    #[test]
    fn trailing_slashes_stripped() {
        let path = ArchivePath::try_from(Path::new("foo/bar/")).unwrap();
        assert_eq!(path.as_str(), Some("foo/bar"));
    }

    /// Mixed normalization.
    #[test]
    fn mixed_normalization() {
        let path = ArchivePath::try_from(Path::new("///foo\\\\bar//baz\\")).unwrap();
        assert_eq!(path.as_str(), Some("foo/bar/baz"));
    }

    /// Simple filename.
    #[test]
    fn simple_filename() {
        let path = ArchivePath::try_from(Path::new("hello.txt")).unwrap();
        assert_eq!(path.as_str(), Some("hello.txt"));
    }

    /// Dot components are removed.
    #[test]
    fn dot_components_removed() {
        let path = ArchivePath::try_from(Path::new("foo/./bar/./baz")).unwrap();
        assert_eq!(path.as_str(), Some("foo/bar/baz"));
    }

    /// Dot-dot components resolve to parent.
    #[test]
    fn dotdot_resolves_parent() {
        let path = ArchivePath::try_from(Path::new("foo/bar/../baz")).unwrap();
        assert_eq!(path.as_str(), Some("foo/baz"));
    }

    /// Multiple dot-dot components.
    #[test]
    fn multiple_dotdot() {
        let path = ArchivePath::try_from(Path::new("foo/bar/baz/../../qux")).unwrap();
        assert_eq!(path.as_str(), Some("foo/qux"));
    }

    /// Dot-dot at start is ignored (can't go above root).
    #[test]
    fn dotdot_at_start_ignored() {
        let path = ArchivePath::try_from(Path::new("../foo/bar")).unwrap();
        assert_eq!(path.as_str(), Some("foo/bar"));
    }

    /// Mixed dot and dot-dot.
    #[test]
    fn mixed_dot_dotdot() {
        let path = ArchivePath::try_from(Path::new("./foo/../bar/./baz")).unwrap();
        assert_eq!(path.as_str(), Some("bar/baz"));
    }

    /// Whitespace is trimmed.
    #[test]
    fn whitespace_trimmed() {
        let path = ArchivePath::try_from("  foo/bar  ").unwrap();
        assert_eq!(path.as_str(), Some("foo/bar"));
    }

    /// Raw bytes can be used to create an ArchivePath.
    #[test]
    fn from_raw_bytes() {
        let path = ArchivePath::from(b"foo/bar".to_vec());
        assert_eq!(path.as_str(), Some("foo/bar"));
        assert_eq!(path.as_bytes(), b"foo/bar");
    }

    /// Invalid UTF-8 bytes return None from as_str but display lossily.
    #[test]
    fn invalid_utf8_display() {
        let path = ArchivePath::from(vec![0x66, 0x6F, 0x6F, 0xFF, 0x62, 0x61, 0x72]); // foo<invalid>bar
        assert_eq!(path.as_str(), None);
        assert!(path.to_string().contains('\u{FFFD}')); // Contains replacement char
    }

    /// ArchivePath can be hashed.
    #[test]
    fn hashable() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(ArchivePath::try_from("foo/bar").unwrap());
        set.insert(ArchivePath::try_from("foo/bar").unwrap()); // Duplicate
        set.insert(ArchivePath::try_from("baz/qux").unwrap());
        assert_eq!(set.len(), 2);
    }

    /// ArchivePath can be sorted.
    #[test]
    fn orderable() {
        let mut paths = [
            ArchivePath::try_from("z/file").unwrap(),
            ArchivePath::try_from("a/file").unwrap(),
            ArchivePath::try_from("m/file").unwrap(),
        ];
        paths.sort();
        assert_eq!(paths[0].as_str(), Some("a/file"));
        assert_eq!(paths[1].as_str(), Some("m/file"));
        assert_eq!(paths[2].as_str(), Some("z/file"));
    }

    /// TryFrom for String works.
    #[test]
    fn try_from_string() {
        let path = ArchivePath::try_from(String::from("foo/bar")).unwrap();
        assert_eq!(path.as_str(), Some("foo/bar"));
    }

    /// TryFrom for PathBuf works.
    #[test]
    fn try_from_pathbuf() {
        let path = ArchivePath::try_from(PathBuf::from("foo/bar")).unwrap();
        assert_eq!(path.as_str(), Some("foo/bar"));
    }

    /// TryFrom for OsString works.
    #[test]
    fn try_from_osstring() {
        let path = ArchivePath::try_from(OsString::from("foo/bar")).unwrap();
        assert_eq!(path.as_str(), Some("foo/bar"));
    }

    /// Into Vec<u8> works.
    #[test]
    fn into_vec_u8() {
        let path = ArchivePath::try_from("foo/bar").unwrap();
        let bytes: Vec<u8> = path.into();
        assert_eq!(bytes, b"foo/bar");
    }
}
