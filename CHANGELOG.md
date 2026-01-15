# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2025-01-14

### Added

#### Core Library

- ZIP-compatible archive format with fixed-stride entries for O(1) random access
- Memory-mapped I/O with shared file locking for concurrent reads
- Append-only writer with automatic alignment (4096 bytes default)
- Configurable path size (1-4096 bytes) and alignment (up to 16 MB)
- ZIP64 support for large archives (>65535 entries, >4GB)
- Stable entry IDs stored in ZIP-compatible extra field
- Path validation via `safename` crate (blocks shell metacharacters, control chars)
- Reserved `.bale*` paths for future format extensions

#### CLI Commands

- `touch` - Create empty archive or update mtime
- `add` - Add files with optional `--prefix`
- `list` - List entries with `ls -alF` style output
- `extract` - Extract entries to directory
- `delete` - Remove entries from archive
- `compact` - Reclaim space and deduplicate paths
- `check` - Verify archive integrity with `--fix` option
- `mount` - FUSE filesystem with read/write support

#### FUSE Filesystem

- Full read/write support (create, modify, delete files)
- Directory operations (mkdir, rmdir)
- Symlink support
- File permissions (chmod) for files, directories, and symlinks
- Timestamp modification (utimensat, mtime)
- Interactive shell mode (`--shell`)
- Background daemon mode (`--background`)
- Read-only mode (`--read-only`)
- Proper error handling (EPERM for chown, ENOSYS for unsupported ops)

### Security

- Safe filename validation blocks path traversal and shell injection
- Reserved paths prevent conflicts with format extensions

[Unreleased]: https://github.com/theroyalwhee0/bale/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/theroyalwhee0/bale/releases/tag/v0.1.0
