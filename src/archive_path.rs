//! Archive path type for validated, normalized paths within a bale archive.

use std::path::Path;

use crate::BaleError;

/// A validated, normalized path within a bale archive.
///
/// Archive paths are always UTF-8 strings with forward slashes as separators.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchivePath(String);

impl ArchivePath {
    /// Creates a new archive path from a string without normalization.
    ///
    /// Use [`ArchivePath::try_from_path`] to create from a filesystem path
    /// with automatic normalization.
    pub fn new(path: impl Into<String>) -> Self {
        Self(path.into())
    }

    /// Creates an archive path from a filesystem path.
    ///
    /// Converts the path to UTF-8 and normalizes it for archive storage.
    ///
    /// # Errors
    ///
    /// Returns [`BaleError::InvalidPath`] if the path is not valid UTF-8.
    pub fn try_from_path(path: impl AsRef<Path>) -> Result<Self, BaleError> {
        let path = path.as_ref();
        let s = path.to_str().ok_or(BaleError::InvalidPath)?;
        Ok(Self(Self::normalize(s)))
    }

    /// Returns the path as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the path as bytes.
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    /// Returns the length in bytes.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns true if the path is empty.
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
    fn normalize(path: &str) -> String {
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

        components.join("/")
    }
}

/// Allows `ArchivePath` to be used where a `&str` is expected.
impl AsRef<str> for ArchivePath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Allows `ArchivePath` to be used where a `&[u8]` is expected.
impl AsRef<[u8]> for ArchivePath {
    fn as_ref(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

/// Consumes the `ArchivePath` and returns the inner `String`.
impl From<ArchivePath> for String {
    fn from(path: ArchivePath) -> Self {
        path.0
    }
}

/// Converts a `&Path` to an `ArchivePath` with normalization.
///
/// Returns `BaleError::InvalidPath` if the path is not valid UTF-8.
impl TryFrom<&Path> for ArchivePath {
    type Error = BaleError;

    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        Self::try_from_path(path)
    }
}

/// Converts an `&OsStr` to an `ArchivePath` with normalization.
///
/// Returns `BaleError::InvalidPath` if the string is not valid UTF-8.
impl<'a> TryFrom<&'a std::ffi::OsStr> for ArchivePath {
    type Error = BaleError;

    fn try_from(s: &'a std::ffi::OsStr) -> Result<Self, Self::Error> {
        Self::try_from_path(Path::new(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Forward slashes are preserved.
    #[test]
    fn forward_slashes_preserved() {
        let path = ArchivePath::new("foo/bar/baz");
        assert_eq!(path.as_str(), "foo/bar/baz");
    }

    /// Backslashes are converted to forward slashes.
    #[test]
    fn backslashes_converted() {
        let path = ArchivePath::try_from_path(Path::new("foo\\bar\\baz")).unwrap();
        assert_eq!(path.as_str(), "foo/bar/baz");
    }

    /// Leading slashes are stripped.
    #[test]
    fn leading_slashes_stripped() {
        let path = ArchivePath::try_from_path(Path::new("/foo/bar")).unwrap();
        assert_eq!(path.as_str(), "foo/bar");
    }

    /// Multiple slashes are collapsed.
    #[test]
    fn multiple_slashes_collapsed() {
        let path = ArchivePath::try_from_path(Path::new("foo//bar///baz")).unwrap();
        assert_eq!(path.as_str(), "foo/bar/baz");
    }

    /// Trailing slashes are stripped.
    #[test]
    fn trailing_slashes_stripped() {
        let path = ArchivePath::try_from_path(Path::new("foo/bar/")).unwrap();
        assert_eq!(path.as_str(), "foo/bar");
    }

    /// Mixed normalization.
    #[test]
    fn mixed_normalization() {
        let path = ArchivePath::try_from_path(Path::new("///foo\\\\bar//baz\\")).unwrap();
        assert_eq!(path.as_str(), "foo/bar/baz");
    }

    /// Simple filename.
    #[test]
    fn simple_filename() {
        let path = ArchivePath::try_from_path(Path::new("hello.txt")).unwrap();
        assert_eq!(path.as_str(), "hello.txt");
    }

    /// Dot components are removed.
    #[test]
    fn dot_components_removed() {
        let path = ArchivePath::try_from_path(Path::new("foo/./bar/./baz")).unwrap();
        assert_eq!(path.as_str(), "foo/bar/baz");
    }

    /// Dot-dot components resolve to parent.
    #[test]
    fn dotdot_resolves_parent() {
        let path = ArchivePath::try_from_path(Path::new("foo/bar/../baz")).unwrap();
        assert_eq!(path.as_str(), "foo/baz");
    }

    /// Multiple dot-dot components.
    #[test]
    fn multiple_dotdot() {
        let path = ArchivePath::try_from_path(Path::new("foo/bar/baz/../../qux")).unwrap();
        assert_eq!(path.as_str(), "foo/qux");
    }

    /// Dot-dot at start is ignored (can't go above root).
    #[test]
    fn dotdot_at_start_ignored() {
        let path = ArchivePath::try_from_path(Path::new("../foo/bar")).unwrap();
        assert_eq!(path.as_str(), "foo/bar");
    }

    /// Mixed dot and dot-dot.
    #[test]
    fn mixed_dot_dotdot() {
        let path = ArchivePath::try_from_path(Path::new("./foo/../bar/./baz")).unwrap();
        assert_eq!(path.as_str(), "bar/baz");
    }

    /// Whitespace is trimmed.
    #[test]
    fn whitespace_trimmed() {
        let path = ArchivePath::new("  foo/bar  ");
        assert_eq!(ArchivePath::normalize(path.as_str()), "foo/bar");
    }
}
