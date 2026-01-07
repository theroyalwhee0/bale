//! FUSE filesystem support for bale archives.
//!
//! This module provides types and constants for mounting bale archives
//! as FUSE filesystems.

mod bale_fs;
mod bale_fs_state;
mod dir_entry;
mod inode;

pub use bale_fs::BaleFs;
pub use dir_entry::FuseDirEntry;
pub use inode::{DIR_INO_START, FILE_INO_START, ROOT_INO, TTL};
