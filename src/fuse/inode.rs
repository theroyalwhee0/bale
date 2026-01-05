//! Inode allocation constants for the FUSE filesystem.

use std::time::Duration;

/// Root directory inode number.
///
/// Per FUSE convention, the root directory is always inode 1.
pub const ROOT_INO: u64 = 1;

/// Starting inode number for regular files.
///
/// Files are allocated inodes starting at 0x1_0000_0000 to separate
/// them from directory inodes in the inode space.
pub const FILE_INO_START: u64 = 0x1_0000_0000;

/// Starting inode number for directories.
///
/// Directories are allocated inodes starting at 0x2_0000_0000 to separate
/// them from file inodes in the inode space.
pub const DIR_INO_START: u64 = 0x2_0000_0000;

/// Time-to-live for cached attributes and entries.
///
/// FUSE caches file attributes and directory entries for this duration
/// before re-querying the filesystem.
pub const TTL: Duration = Duration::from_secs(1);
