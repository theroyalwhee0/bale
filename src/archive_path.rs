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

    /// Returns the length of the path in bytes.
    ///
    /// This is the byte length, not the number of path components or characters.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns true if the path has zero bytes.
    ///
    /// Note: Paths created via [`TryFrom`] are never empty (empty paths are rejected).
    /// This can only be true for paths created via [`From<Vec<u8>>`] with an empty vector.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Normalizes a path string for archive storage.
    ///
    /// - Trims leading/trailing whitespace from the entire path
    /// - Converts backslashes to forward slashes
    /// - Removes `.` components
    /// - Resolves `..` components (errors if escaping root)
    /// - Removes leading/trailing slashes
    /// - Collapses multiple slashes
    ///
    /// Whitespace within path components is preserved (spaces in filenames are valid).
    /// For example, `"foo/ bar /baz"` normalizes to `"foo/ bar /baz"`.
    ///
    /// # Errors
    ///
    /// Returns `BaleError::InvalidPath` if:
    /// - The path attempts to escape the archive root (e.g., `../etc/passwd`)
    /// - The path is empty after normalization
    fn normalize(path: &str) -> Result<Vec<u8>, BaleError> {
        let mut components: Vec<&str> = Vec::new();

        for part in path.trim().split(['/', '\\']) {
            match part {
                "" | "." => {}
                ".." => {
                    if components.pop().is_none() {
                        // Attempted to go above root - path traversal attack
                        return Err(BaleError::InvalidPath);
                    }
                }
                component => {
                    components.push(component);
                }
            }
        }

        if components.is_empty() {
            return Err(BaleError::InvalidPath);
        }

        Ok(components.join("/").into_bytes())
    }

    /// Returns a displayable wrapper that avoids allocation for valid UTF-8 paths.
    ///
    /// For paths that are valid UTF-8, this writes directly without copying.
    /// For paths with invalid UTF-8, this falls back to lossy conversion.
    #[must_use]
    pub fn display(&self) -> Display<'_> {
        Display(self)
    }
}

/// A wrapper for displaying an [`ArchivePath`] efficiently.
///
/// This type avoids allocation when the path is valid UTF-8, falling back to
/// lossy conversion only when necessary.
///
/// Created by [`ArchivePath::display`].
pub struct Display<'a>(&'a ArchivePath);

impl fmt::Display for Display<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0.as_str() {
            Some(s) => f.write_str(s),
            None => write!(f, "{}", String::from_utf8_lossy(&self.0.0)),
        }
    }
}

/// Displays the path, using lossy UTF-8 conversion for invalid bytes.
///
/// This delegates to [`Display`] which avoids allocation for valid UTF-8 paths.
impl fmt::Display for ArchivePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.display(), f)
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
/// Use this when reading paths from an existing archive. The bytes are stored
/// as-is and may contain:
/// - Non-UTF-8 sequences
/// - Backslashes (not converted to forward slashes)
/// - `..` or `.` components (not resolved)
/// - Leading or trailing slashes (not stripped)
///
/// Paths created this way may compare differently than equivalent paths created
/// via [`TryFrom`], which normalizes the input. For user-supplied paths, prefer
/// the `TryFrom` implementations.
impl From<Vec<u8>> for ArchivePath {
    fn from(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

/// Creates an `ArchivePath` from raw bytes without validation.
///
/// Use this when reading paths from an existing archive. The bytes are stored
/// as-is and may contain:
/// - Non-UTF-8 sequences
/// - Backslashes (not converted to forward slashes)
/// - `..` or `.` components (not resolved)
/// - Leading or trailing slashes (not stripped)
///
/// Paths created this way may compare differently than equivalent paths created
/// via [`TryFrom`], which normalizes the input. For user-supplied paths, prefer
/// the `TryFrom` implementations.
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
///
/// # Errors
///
/// Returns `BaleError::InvalidPath` if the path is empty or attempts
/// to escape the archive root via `..` components.
impl TryFrom<&str> for ArchivePath {
    type Error = BaleError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Ok(Self(Self::normalize(s)?))
    }
}

/// Converts a `String` to an `ArchivePath` with normalization.
///
/// # Errors
///
/// Returns `BaleError::InvalidPath` if the path is empty or attempts
/// to escape the archive root via `..` components.
impl TryFrom<String> for ArchivePath {
    type Error = BaleError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Ok(Self(Self::normalize(&s)?))
    }
}

/// Converts a `&Path` to an `ArchivePath` with normalization.
///
/// # Errors
///
/// Returns `BaleError::InvalidPath` if the path is not valid UTF-8,
/// is empty, or attempts to escape the archive root via `..` components.
impl TryFrom<&Path> for ArchivePath {
    type Error = BaleError;

    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        let s = path.to_str().ok_or(BaleError::InvalidPath)?;
        Ok(Self(Self::normalize(s)?))
    }
}

/// Converts a `PathBuf` to an `ArchivePath` with normalization.
///
/// # Errors
///
/// Returns `BaleError::InvalidPath` if the path is not valid UTF-8,
/// is empty, or attempts to escape the archive root via `..` components.
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
/// Returns `BaleError::InvalidPath` if the string is not valid UTF-8,
/// is empty, or attempts to escape the archive root via `..` components.
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
/// Returns `BaleError::InvalidPath` if the string is not valid UTF-8,
/// is empty, or attempts to escape the archive root via `..` components.
impl TryFrom<OsString> for ArchivePath {
    type Error = BaleError;

    fn try_from(s: OsString) -> Result<Self, Self::Error> {
        Self::try_from(s.as_os_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // ==================== Unit Tests ====================

    /// Backslashes are converted to forward slashes.
    #[test]
    fn backslashes_converted() {
        let path = ArchivePath::try_from("foo\\bar\\baz").unwrap();
        assert_eq!(path.as_str(), Some("foo/bar/baz"));
    }

    /// Dot-dot components resolve to parent.
    #[test]
    fn dotdot_resolves_parent() {
        let path = ArchivePath::try_from("foo/bar/../baz").unwrap();
        assert_eq!(path.as_str(), Some("foo/baz"));
    }

    /// Mixed slashes are normalized before resolving dot-dot.
    #[test]
    fn mixed_slashes_with_dotdot() {
        let path = ArchivePath::try_from("foo/bar\\baz/../qux").unwrap();
        assert_eq!(path.as_str(), Some("foo/bar/qux"));
    }

    /// Raw bytes can be used to create an ArchivePath (no validation).
    #[test]
    fn from_raw_bytes() {
        let path = ArchivePath::from(b"foo/bar".to_vec());
        assert_eq!(path.as_str(), Some("foo/bar"));
        assert_eq!(path.as_bytes(), b"foo/bar");
    }

    /// Invalid UTF-8 bytes return None from as_str but display lossily.
    #[test]
    fn invalid_utf8_display() {
        let path = ArchivePath::from(vec![0x66, 0x6F, 0x6F, 0xFF, 0x62, 0x61, 0x72]);
        assert_eq!(path.as_str(), None);
        assert!(path.to_string().contains('\u{FFFD}'));
    }

    /// ArchivePath can be hashed.
    #[test]
    fn hashable() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(ArchivePath::try_from("foo/bar").unwrap());
        set.insert(ArchivePath::try_from("foo/bar").unwrap());
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

    // ==================== Property Tests ====================

    /// Strategy for valid path component (excludes . and ..).
    fn valid_component() -> impl Strategy<Value = String> {
        // Start with alphanumeric, then allow alphanumeric + safe chars
        "[a-zA-Z0-9][a-zA-Z0-9_.-]{0,19}"
    }

    /// Strategy for valid paths (1-5 components joined by /).
    fn valid_path() -> impl Strategy<Value = String> {
        prop::collection::vec(valid_component(), 1..=5).prop_map(|components| components.join("/"))
    }

    proptest! {
        /// Valid paths are accepted and produce non-empty results.
        #[test]
        fn valid_paths_accepted(path in valid_path()) {
            let result = ArchivePath::try_from(path.as_str());
            prop_assert!(result.is_ok(), "valid path rejected: {}", path);
            let archive_path = result.unwrap();
            prop_assert!(!archive_path.is_empty());
            prop_assert!(archive_path.as_str().is_some());
        }

        /// Normalized paths have no consecutive slashes.
        #[test]
        fn no_consecutive_slashes(path in valid_path()) {
            let archive_path = ArchivePath::try_from(path.as_str()).unwrap();
            let s = archive_path.as_str().unwrap();
            prop_assert!(!s.contains("//"), "consecutive slashes in: {}", s);
        }

        /// Normalized paths have no leading or trailing slashes.
        #[test]
        fn no_leading_trailing_slashes(path in valid_path()) {
            let archive_path = ArchivePath::try_from(path.as_str()).unwrap();
            let s = archive_path.as_str().unwrap();
            prop_assert!(!s.starts_with('/'), "leading slash in: {}", s);
            prop_assert!(!s.ends_with('/'), "trailing slash in: {}", s);
        }

        /// Paths with leading slashes are normalized (slashes stripped).
        #[test]
        fn leading_slashes_stripped(
            slashes in "/+",
            path in valid_path(),
        ) {
            let input = format!("{}{}", slashes, path);
            let result = ArchivePath::try_from(input.as_str());
            prop_assert!(result.is_ok());
            let s = result.unwrap().as_str().unwrap().to_string();
            prop_assert!(!s.starts_with('/'));
        }

        /// Paths with trailing slashes are normalized (slashes stripped).
        #[test]
        fn trailing_slashes_stripped(
            path in valid_path(),
            slashes in "/+",
        ) {
            let input = format!("{}{}", path, slashes);
            let result = ArchivePath::try_from(input.as_str());
            prop_assert!(result.is_ok());
            let s = result.unwrap().as_str().unwrap().to_string();
            prop_assert!(!s.ends_with('/'));
        }

        /// Path traversal attempts (leading ..) are rejected.
        #[test]
        fn path_traversal_rejected(
            num_dotdots in 1usize..=4,
            suffix in prop::option::of(valid_path()),
        ) {
            let dotdots = vec![".."; num_dotdots].join("/");
            let input = match suffix {
                Some(s) => format!("{}/{}", dotdots, s),
                None => dotdots,
            };
            let result = ArchivePath::try_from(input.as_str());
            prop_assert!(result.is_err(), "path traversal accepted: {}", input);
        }

        /// Paths that resolve to empty are rejected.
        #[test]
        fn empty_paths_rejected(
            input in prop_oneof![
                Just("".to_string()),
                Just(".".to_string()),
                Just("/".to_string()),
                Just("   ".to_string()),
                Just("///".to_string()),
                Just("./././".to_string()),
                Just("./.".to_string()),
            ],
        ) {
            let result = ArchivePath::try_from(input.as_str());
            prop_assert!(result.is_err(), "empty path accepted: {:?}", input);
        }

        /// Too many .. components (escaping root) are rejected.
        #[test]
        fn excess_dotdot_rejected(
            components in prop::collection::vec(valid_component(), 1..=3),
            extra_dotdots in 1usize..=3,
        ) {
            // Build path like "a/b/c" then add more ".." than components
            let mut path = components.join("/");
            for _ in 0..components.len() + extra_dotdots {
                path.push_str("/..");
            }
            let result = ArchivePath::try_from(path.as_str());
            prop_assert!(result.is_err(), "excess dotdot accepted: {}", path);
        }

        /// TryFrom works for all string-like types.
        #[test]
        fn tryfrom_all_types(path in valid_path()) {
            // &str
            prop_assert!(ArchivePath::try_from(path.as_str()).is_ok());
            // String
            prop_assert!(ArchivePath::try_from(path.clone()).is_ok());
            // &Path
            prop_assert!(ArchivePath::try_from(Path::new(&path)).is_ok());
            // PathBuf
            prop_assert!(ArchivePath::try_from(PathBuf::from(&path)).is_ok());
            // &OsStr
            prop_assert!(ArchivePath::try_from(OsStr::new(&path)).is_ok());
            // OsString
            prop_assert!(ArchivePath::try_from(OsString::from(&path)).is_ok());
        }
    }
}
