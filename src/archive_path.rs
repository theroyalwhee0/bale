//! Archive path type for validated, normalized paths within a bale archive.

use std::borrow::{Borrow, Cow};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};

use crate::BaleError;

/// A path within a bale archive, either borrowed or owned.
///
/// Archive paths use forward slashes as separators and are typically UTF-8,
/// but may contain arbitrary bytes when read from an archive.
///
/// # Lifetimes
///
/// - `ArchivePath<'a>` borrows bytes (zero-copy from mmap)
/// - `ArchivePath<'static>` owns bytes (for storage or modification)
///
/// # Construction
///
/// - Use [`from_bytes`](Self::from_bytes) for zero-copy wrapping of archive data
/// - Use [`TryFrom<&str>`], [`TryFrom<&Path>`], etc. for user input (normalizes, always owned)
///
/// # Ordering
///
/// The `Ord` implementation compares bytes lexicographically (ASCII order).
/// This means uppercase letters sort before lowercase (`"A/b"` < `"a/b"`).
/// Ordering is case-sensitive and not locale-aware.
///
/// # Display
///
/// The `Display` implementation uses lossy UTF-8 conversion, replacing invalid
/// bytes with the Unicode replacement character.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArchivePath<'a>(Cow<'a, [u8]>);

impl<'a> ArchivePath<'a> {
    /// Returns the path as a string slice if it is valid UTF-8.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        self.to_str_checked().ok()
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
    /// This can only be true for paths created via [`from_bytes`](Self::from_bytes)
    /// with empty bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Creates an `ArchivePath` by borrowing raw bytes without validation.
    ///
    /// This is zero-copy when borrowing from an mmap'd archive. The bytes are
    /// stored as-is and may contain:
    /// - Non-UTF-8 sequences
    /// - Backslashes (not converted to forward slashes)
    /// - `..` or `.` components (not resolved)
    /// - Leading or trailing slashes (not stripped)
    ///
    /// Paths created this way may compare differently than equivalent paths created
    /// via [`TryFrom`], which normalizes the input. Use [`into_normalized`](Self::into_normalized)
    /// to normalize an archive-read path for comparison. For user-supplied paths,
    /// prefer the [`TryFrom`] implementations.
    #[must_use]
    pub fn from_bytes(bytes: &'a [u8]) -> Self {
        Self(Cow::Borrowed(bytes))
    }

    /// Creates an `ArchivePath` by borrowing bytes, trimming null padding.
    ///
    /// Archive entries use fixed-size path fields padded with null bytes.
    /// This constructor trims trailing nulls before storing.
    #[must_use]
    pub fn from_null_padded_bytes(bytes: &'a [u8]) -> Self {
        let trimmed = bytes
            .iter()
            .position(|&b| b == 0)
            .map_or(bytes, |pos| &bytes[..pos]);
        Self(Cow::Borrowed(trimmed))
    }

    /// Returns the path as a string slice, or an error if not valid UTF-8.
    ///
    /// Use this when invalid UTF-8 should be treated as an error rather than
    /// silently ignored. For optional access, use [`as_str`](Self::as_str).
    ///
    /// # Errors
    ///
    /// Returns `BaleError::InvalidUtf8` if the path is not valid UTF-8.
    pub fn to_str_checked(&self) -> Result<&str, BaleError> {
        Ok(std::str::from_utf8(&self.0)?)
    }

    /// Returns the filename component as bytes (after the last `/`).
    ///
    /// For a path like `foo/bar/baz.txt`, returns `baz.txt`.
    /// For a path with no slashes, returns the entire path.
    ///
    /// This method works on raw bytes. For UTF-8 paths, use
    /// [`file_name_str`](Self::file_name_str) instead.
    #[must_use]
    pub fn file_name(&self) -> &[u8] {
        self.0
            .iter()
            .rposition(|&b| b == b'/')
            .map_or(&self.0[..], |pos| &self.0[pos + 1..])
    }

    /// Returns the filename component as a string slice.
    ///
    /// For a path like `foo/bar/baz.txt`, returns `baz.txt`.
    /// For a path with no slashes, returns the entire path.
    ///
    /// # Errors
    ///
    /// Returns `BaleError::InvalidUtf8` if the path is not valid UTF-8.
    pub fn file_name_str(&self) -> Result<&str, BaleError> {
        Ok(std::str::from_utf8(self.file_name())?)
    }

    /// Returns a new path with a numeric suffix inserted before the extension.
    ///
    /// Only considers the filename component when looking for extensions,
    /// so `foo.d/bar` correctly treats `bar` as having no extension.
    ///
    /// The returned path is already normalized (no trailing slashes, forward
    /// slashes only) if the input was normalized.
    ///
    /// # Path Size
    ///
    /// This method does not check whether the result fits within the archive's
    /// `path_size`. Callers should verify `result.len() <= path_size` before
    /// writing to an archive.
    ///
    /// # Examples
    ///
    /// - `file.txt` + 1 → `file(1).txt`
    /// - `dir/file.txt` + 2 → `dir/file(2).txt`
    /// - `foo.d/bar` + 1 → `foo.d/bar(1)`
    /// - `noext` + 3 → `noext(3)`
    ///
    /// # Errors
    ///
    /// Returns `BaleError::InvalidUtf8` if the path is not valid UTF-8.
    pub fn with_suffix(&self, number: usize) -> Result<ArchivePath<'static>, BaleError> {
        let path = self.to_str_checked()?;

        // Find where the filename starts (after the last slash).
        let filename_start = path.rfind('/').map_or(0, |pos| pos + 1);
        let (dir, filename) = path.split_at(filename_start);

        // Look for extension only within the filename.
        let result = if let Some(dot_pos) = filename.rfind('.') {
            // Has extension: insert suffix before the last dot in filename.
            let (stem, ext) = filename.split_at(dot_pos);
            format!("{dir}{stem}({number}){ext}")
        } else {
            // No extension: append suffix at the end.
            format!("{path}({number})")
        };

        Ok(ArchivePath(Cow::Owned(result.into_bytes())))
    }

    /// Converts this path into an owned version with `'static` lifetime.
    ///
    /// If the path is already owned, this is a no-op. If borrowed, this clones
    /// the underlying bytes.
    #[must_use]
    pub fn into_owned(self) -> ArchivePath<'static> {
        ArchivePath(Cow::Owned(self.0.into_owned()))
    }

    /// Returns a normalized, owned copy of this path.
    ///
    /// This is useful for paths created via [`from_bytes`](Self::from_bytes) that may contain
    /// backslashes, `..` components, or other non-normalized content. After
    /// normalization, the path can be compared with paths created via [`TryFrom`].
    ///
    /// Use [`into_normalized`](Self::into_normalized) if you don't need to keep the original.
    ///
    /// # Errors
    ///
    /// Returns `BaleError::InvalidPath` if:
    /// - The path is not valid UTF-8
    /// - The path attempts to escape the archive root (e.g., `../etc/passwd`)
    /// - The path is empty after normalization
    pub fn normalize(&self) -> Result<ArchivePath<'static>, BaleError> {
        let s = self.as_str().ok_or(BaleError::InvalidPath)?;
        Ok(ArchivePath(Cow::Owned(
            Self::normalize_bytes(s)?.into_owned(),
        )))
    }

    /// Consumes this path and returns a normalized, owned version.
    ///
    /// This is equivalent to [`normalize`](Self::normalize) but consumes `self`.
    /// Use this when you don't need to keep the original path.
    ///
    /// # Errors
    ///
    /// Returns `BaleError::InvalidPath` if:
    /// - The path is not valid UTF-8
    /// - The path attempts to escape the archive root (e.g., `../etc/passwd`)
    /// - The path is empty after normalization
    pub fn into_normalized(self) -> Result<ArchivePath<'static>, BaleError> {
        self.normalize()
    }

    /// Normalizes a path string for archive storage.
    ///
    /// - Trims leading/trailing whitespace from the entire path
    /// - Converts backslashes to forward slashes
    /// - Removes `.` components
    /// - Resolves `..` components (errors if escaping root)
    /// - Removes leading/trailing slashes
    /// - Collapses multiple slashes
    /// - Rejects whitespace-only components (e.g., `"foo/ /bar"`)
    ///
    /// Whitespace within path components is preserved (spaces in filenames are valid).
    /// For example, `"foo/ bar /baz"` normalizes to `"foo/ bar /baz"`.
    ///
    /// Returns a borrowed slice if the path is already normalized, avoiding allocation.
    ///
    /// # Errors
    ///
    /// Returns `BaleError::InvalidPath` if:
    /// - The path attempts to escape the archive root (e.g., `../etc/passwd`)
    /// - The path is empty after normalization
    /// - The path contains whitespace-only components
    fn normalize_bytes(path: &str) -> Result<Cow<'_, [u8]>, BaleError> {
        let trimmed = path.trim();

        // Fast path: check if already normalized.
        if Self::is_normalized(trimmed) {
            return Ok(Cow::Borrowed(trimmed.as_bytes()));
        }

        // Slow path: build normalized version.
        let mut components: Vec<&str> = Vec::new();

        for part in trimmed.split(['/', '\\']) {
            match part {
                "" | "." => {}
                ".." => {
                    if components.pop().is_none() {
                        // Attempted to go above root - path traversal attack
                        return Err(BaleError::InvalidPath);
                    }
                }
                component if component.trim().is_empty() => {
                    // Whitespace-only component - reject
                    return Err(BaleError::InvalidPath);
                }
                component => {
                    components.push(component);
                }
            }
        }

        if components.is_empty() {
            return Err(BaleError::InvalidPath);
        }

        Ok(Cow::Owned(components.join("/").into_bytes()))
    }

    /// Checks if a path string is already in normalized form.
    ///
    /// A normalized path has:
    /// - No backslashes
    /// - No leading or trailing slashes
    /// - No consecutive slashes
    /// - No `.` or `..` components
    /// - At least one component
    fn is_normalized(path: &str) -> bool {
        // Must not be empty.
        if path.is_empty() {
            return false;
        }

        // No backslashes.
        if path.contains('\\') {
            return false;
        }

        // No leading or trailing slashes.
        if path.starts_with('/') || path.ends_with('/') {
            return false;
        }

        // Check each component.
        for component in path.split('/') {
            // No empty, whitespace-only, or special components.
            if component.is_empty()
                || component.trim().is_empty()
                || component == "."
                || component == ".."
            {
                return false;
            }
        }

        true
    }
}

/// Displays the path, using lossy UTF-8 conversion for invalid bytes.
///
/// Lossy conversion is acceptable here because Display is for human-readable
/// output only. Code that needs exact byte content should use `as_bytes()`.
#[allow(clippy::disallowed_methods)]
impl fmt::Display for ArchivePath<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", String::from_utf8_lossy(&self.0))
    }
}

/// Allows `ArchivePath` to be used where a `&[u8]` is expected.
impl AsRef<[u8]> for ArchivePath<'_> {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// Allows `ArchivePath` to be used as a `HashMap` key with `&[u8]` lookups.
impl Borrow<[u8]> for ArchivePath<'_> {
    fn borrow(&self) -> &[u8] {
        &self.0
    }
}

/// Creates an owned `ArchivePath` from a `Vec<u8>`.
///
/// See [`ArchivePath::from_bytes`] for details on raw byte paths.
impl From<Vec<u8>> for ArchivePath<'static> {
    fn from(bytes: Vec<u8>) -> Self {
        Self(Cow::Owned(bytes))
    }
}

/// Creates a borrowed `ArchivePath` from a byte slice.
///
/// See [`ArchivePath::from_bytes`] for details on raw byte paths.
impl<'a> From<&'a [u8]> for ArchivePath<'a> {
    fn from(bytes: &'a [u8]) -> Self {
        Self::from_bytes(bytes)
    }
}

/// Consumes the `ArchivePath` and returns the inner bytes as owned.
impl From<ArchivePath<'_>> for Vec<u8> {
    fn from(path: ArchivePath<'_>) -> Self {
        path.0.into_owned()
    }
}

/// Converts a `&str` to an owned `ArchivePath` with normalization.
///
/// # Errors
///
/// Returns `BaleError::InvalidPath` if the path is empty, contains
/// whitespace-only components, or attempts to escape the archive root
/// via `..` components.
impl TryFrom<&str> for ArchivePath<'static> {
    type Error = BaleError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Ok(Self(Cow::Owned(Self::normalize_bytes(s)?.into_owned())))
    }
}

/// Converts a `String` to an owned `ArchivePath` with normalization.
///
/// # Errors
///
/// Returns `BaleError::InvalidPath` if the path is empty, contains
/// whitespace-only components, or attempts to escape the archive root
/// via `..` components.
impl TryFrom<String> for ArchivePath<'static> {
    type Error = BaleError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Ok(Self(Cow::Owned(Self::normalize_bytes(&s)?.into_owned())))
    }
}

/// Converts a `&Path` to an owned `ArchivePath` with normalization.
///
/// # Errors
///
/// Returns `BaleError::InvalidPath` if the path is not valid UTF-8,
/// is empty, contains whitespace-only components, or attempts to escape
/// the archive root via `..` components.
impl TryFrom<&Path> for ArchivePath<'static> {
    type Error = BaleError;

    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        let s = path.to_str().ok_or(BaleError::InvalidPath)?;
        Ok(Self(Cow::Owned(Self::normalize_bytes(s)?.into_owned())))
    }
}

/// Converts a `PathBuf` to an owned `ArchivePath` with normalization.
///
/// # Errors
///
/// Returns `BaleError::InvalidPath` if the path is not valid UTF-8,
/// is empty, contains whitespace-only components, or attempts to escape
/// the archive root via `..` components.
impl TryFrom<PathBuf> for ArchivePath<'static> {
    type Error = BaleError;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        ArchivePath::try_from(path.as_path())
    }
}

/// Converts an `&OsStr` to an owned `ArchivePath` with normalization.
///
/// # Errors
///
/// Returns `BaleError::InvalidPath` if the string is not valid UTF-8,
/// is empty, contains whitespace-only components, or attempts to escape
/// the archive root via `..` components.
impl TryFrom<&OsStr> for ArchivePath<'static> {
    type Error = BaleError;

    fn try_from(s: &OsStr) -> Result<Self, Self::Error> {
        ArchivePath::try_from(Path::new(s))
    }
}

/// Converts an `OsString` to an owned `ArchivePath` with normalization.
///
/// # Errors
///
/// Returns `BaleError::InvalidPath` if the string is not valid UTF-8,
/// is empty, contains whitespace-only components, or attempts to escape
/// the archive root via `..` components.
impl TryFrom<OsString> for ArchivePath<'static> {
    type Error = BaleError;

    fn try_from(s: OsString) -> Result<Self, Self::Error> {
        ArchivePath::try_from(s.as_os_str())
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

    /// from_bytes borrows without copying.
    #[test]
    fn from_bytes_borrows() {
        let bytes = b"foo/bar";
        let path = ArchivePath::from_bytes(bytes);
        // Verify it points to the same memory
        assert!(std::ptr::eq(path.as_bytes().as_ptr(), bytes.as_ptr()));
    }

    /// From<&[u8]> borrows without copying (same as from_bytes).
    #[test]
    fn from_slice_borrows() {
        let bytes: &[u8] = b"foo/bar";
        let path = ArchivePath::from(bytes);
        // Verify it points to the same memory
        assert!(std::ptr::eq(path.as_bytes().as_ptr(), bytes.as_ptr()));
    }

    /// into_owned converts borrowed to owned.
    #[test]
    fn into_owned_works() {
        let bytes = b"foo/bar";
        let borrowed = ArchivePath::from_bytes(bytes);
        let owned = borrowed.into_owned();
        assert_eq!(owned.as_str(), Some("foo/bar"));
        // Owned path no longer points to original bytes
        assert!(!std::ptr::eq(owned.as_bytes().as_ptr(), bytes.as_ptr()));
    }

    /// into_normalized normalizes a raw-bytes path for comparison.
    #[test]
    fn into_normalized_works() {
        let raw = ArchivePath::from(b"foo\\bar/../baz".to_vec());
        let user = ArchivePath::try_from("foo/baz").unwrap();
        // Before normalization, they differ
        assert_ne!(raw, user);
        // After normalization, they match
        assert_eq!(raw.into_normalized().unwrap(), user);
    }

    /// is_normalized returns true for already-normalized paths.
    #[test]
    fn is_normalized_detects_normalized() {
        assert!(ArchivePath::is_normalized("foo"));
        assert!(ArchivePath::is_normalized("foo/bar"));
        assert!(ArchivePath::is_normalized("foo/bar/baz"));
        assert!(ArchivePath::is_normalized("a/b/c/d/e"));
    }

    /// is_normalized returns false for paths needing normalization.
    #[test]
    fn is_normalized_detects_unnormalized() {
        // Backslashes
        assert!(!ArchivePath::is_normalized("foo\\bar"));
        // Leading slash
        assert!(!ArchivePath::is_normalized("/foo"));
        // Trailing slash
        assert!(!ArchivePath::is_normalized("foo/"));
        // Consecutive slashes
        assert!(!ArchivePath::is_normalized("foo//bar"));
        // Dot component
        assert!(!ArchivePath::is_normalized("foo/./bar"));
        // Dot-dot component
        assert!(!ArchivePath::is_normalized("foo/../bar"));
        // Empty
        assert!(!ArchivePath::is_normalized(""));
        // Whitespace-only component
        assert!(!ArchivePath::is_normalized("foo/ /bar"));
        assert!(!ArchivePath::is_normalized("foo/\t/bar"));
        assert!(!ArchivePath::is_normalized(" "));
    }

    /// normalize_bytes returns borrowed for already-normalized paths.
    #[test]
    fn normalize_bytes_borrows_when_unchanged() {
        let input = "foo/bar/baz";
        let result = ArchivePath::normalize_bytes(input).unwrap();
        // Should be borrowed, not owned
        assert!(matches!(result, Cow::Borrowed(_)));
        // Should point to the input's bytes
        assert!(std::ptr::eq(result.as_ref().as_ptr(), input.as_ptr()));
    }

    /// normalize_bytes returns owned for paths needing changes.
    #[test]
    fn normalize_bytes_copies_when_changed() {
        let result = ArchivePath::normalize_bytes("foo\\bar").unwrap();
        assert!(matches!(result, Cow::Owned(_)));
        assert_eq!(result.as_ref(), b"foo/bar");
    }

    /// normalize_bytes rejects whitespace-only components.
    #[test]
    fn whitespace_only_components_rejected() {
        assert!(ArchivePath::try_from("foo/ /bar").is_err());
        assert!(ArchivePath::try_from("foo/\t/bar").is_err());
        assert!(ArchivePath::try_from("foo/   /bar").is_err());
        // But whitespace around content is allowed
        assert!(ArchivePath::try_from("foo/ bar /baz").is_ok());
    }

    /// into_normalized rejects invalid UTF-8.
    #[test]
    fn into_normalized_rejects_invalid_utf8() {
        let path = ArchivePath::from(vec![0xFF, 0xFE]);
        assert!(path.into_normalized().is_err());
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

    /// HashMap can be looked up by &[u8] via Borrow trait.
    #[test]
    fn hashmap_lookup_by_bytes() {
        use std::collections::HashMap;
        let mut map: HashMap<ArchivePath<'static>, u32> = HashMap::new();
        map.insert(ArchivePath::try_from("foo/bar").unwrap(), 42);
        map.insert(ArchivePath::try_from("baz/qux").unwrap(), 99);

        // Lookup using &[u8] via Borrow<[u8]>
        assert_eq!(map.get(b"foo/bar".as_slice()), Some(&42));
        assert_eq!(map.get(b"baz/qux".as_slice()), Some(&99));
        assert_eq!(map.get(b"nonexistent".as_slice()), None);
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

    /// len returns the byte length of the path.
    #[test]
    fn len_returns_byte_count() {
        let path = ArchivePath::try_from("foo/bar").unwrap();
        assert_eq!(path.len(), 7);
        assert_eq!(path.as_bytes().len(), path.len());
    }

    /// is_empty returns true only for empty paths.
    #[test]
    fn is_empty_works() {
        let empty = ArchivePath::from_bytes(b"");
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);

        let nonempty = ArchivePath::try_from("foo").unwrap();
        assert!(!nonempty.is_empty());
    }

    /// as_bytes returns the underlying bytes.
    #[test]
    fn as_bytes_returns_underlying() {
        let path = ArchivePath::try_from("foo/bar").unwrap();
        assert_eq!(path.as_bytes(), b"foo/bar");
    }

    /// AsRef<[u8]> trait implementation.
    #[test]
    fn asref_bytes() {
        let path = ArchivePath::try_from("foo/bar").unwrap();
        let bytes: &[u8] = path.as_ref();
        assert_eq!(bytes, b"foo/bar");
    }

    /// Borrow<[u8]> trait implementation.
    #[test]
    fn borrow_bytes() {
        use std::borrow::Borrow;
        let path = ArchivePath::try_from("foo/bar").unwrap();
        let bytes: &[u8] = path.borrow();
        assert_eq!(bytes, b"foo/bar");
    }

    /// From<Vec<u8>> creates owned path.
    #[test]
    fn from_vec() {
        let path = ArchivePath::from(b"foo/bar".to_vec());
        assert_eq!(path.as_bytes(), b"foo/bar");
    }

    /// Vec<u8> can be created from ArchivePath.
    #[test]
    fn into_vec() {
        let path = ArchivePath::try_from("foo/bar").unwrap();
        let vec: Vec<u8> = path.into();
        assert_eq!(vec, b"foo/bar");
    }

    /// Display trait shows lossy UTF-8.
    #[test]
    fn display_valid_utf8() {
        let path = ArchivePath::try_from("foo/bar").unwrap();
        assert_eq!(format!("{}", path), "foo/bar");
    }

    /// TryFrom<String> works.
    #[test]
    fn tryfrom_string() {
        let path = ArchivePath::try_from(String::from("foo/bar")).unwrap();
        assert_eq!(path.as_str(), Some("foo/bar"));
    }

    /// TryFrom<PathBuf> works.
    #[test]
    fn tryfrom_pathbuf() {
        let path = ArchivePath::try_from(PathBuf::from("foo/bar")).unwrap();
        assert_eq!(path.as_str(), Some("foo/bar"));
    }

    /// TryFrom<OsString> works.
    #[test]
    fn tryfrom_osstring() {
        let path = ArchivePath::try_from(OsString::from("foo/bar")).unwrap();
        assert_eq!(path.as_str(), Some("foo/bar"));
    }

    /// normalize method works on borrowed paths.
    #[test]
    fn normalize_method() {
        let path = ArchivePath::from_bytes(b"foo\\bar");
        let normalized = path.normalize().unwrap();
        assert_eq!(normalized.as_str(), Some("foo/bar"));
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

    /// Strategy for valid path component that may contain interior spaces.
    fn component_with_spaces() -> impl Strategy<Value = String> {
        "[a-zA-Z0-9][a-zA-Z0-9 _.-]{0,19}"
            .prop_filter("must not be whitespace-only", |s| !s.trim().is_empty())
    }

    /// Strategy for paths with components that may contain interior spaces.
    fn path_with_spaces() -> impl Strategy<Value = String> {
        prop::collection::vec(component_with_spaces(), 1..=5)
            .prop_map(|components| components.join("/"))
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

        /// Components with interior spaces are accepted.
        #[test]
        fn interior_spaces_accepted(path in path_with_spaces()) {
            let result = ArchivePath::try_from(path.as_str());
            prop_assert!(result.is_ok(), "path with interior spaces rejected: {:?}", path);
            let archive_path = result.unwrap();
            prop_assert!(!archive_path.is_empty());
            prop_assert!(archive_path.as_str().is_some());
        }
    }
}
