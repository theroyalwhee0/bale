//! FUSE filesystem support for bale archives.
//!
//! This module provides types and constants for mounting bale archives
//! as read-only FUSE filesystems.

mod dir_entry;
mod inode;

pub use dir_entry::FuseDirEntry;
pub use inode::{DIR_INO_START, FILE_INO_START, ROOT_INO, TTL};
