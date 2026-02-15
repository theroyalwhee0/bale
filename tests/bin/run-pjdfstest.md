# pjdfstest Results

Last run: **2026-02-15** (commit `82a0d79`)

## How to Run

```bash
cargo build --release
sudo tests/bin/run-pjdfstest.sh
```

Requires the pjdfstest suite at `~/projects/ref/pjdfstest/`.
Results are written to `~/projects/pjdfstest-results/`.

## Test Groups

The script splits pjdfstest into three groups:

| Group | Files | Tests | Pass | Fail | Result |
|-------|------:|------:|-----:|-----:|--------|
| **PASS** (should all pass) | 134 | 741 | 741 | 0 | PASS |
| **KNOWN FAILURES** (limitations) | 45 | 6223 | 3567 | 2656 | FAIL |
| **UNSUPPORTED** (not implemented) | 51 | 1818 | 771 | 1047 | FAIL |
| **Total** | 230 | 8782 | 5079 | 3703 | **58%** |

### PASS Group (134 files)

All tests pass. Any failure here is a regression. Covers:

- ftruncate/truncate (01-04, 07-14)
- chmod (02-10, 12)
- link (02-09, 11-17)
- mkdir (02-09, 11-12)
- open (02-05, 07-16, 18-21, 23, 25)
- rename (01-08, 11, 15-19, 21-22)
- rmdir (02-05, 07-15)
- symlink (01, 03-07, 09-12)
- unlink (01-10, 12-13)
- utimensat (01, 03, 06-07)

### KNOWN FAILURES Group (45 files)

Tests that fail due to known limitations. Tracked for improvement.

#### 1. Unsupported node types in test setup (mkfifo/mknod/bind ENOSYS)

The dominant failure pattern. A test creates a FIFO, block device, char
device, or Unix socket as setup, it fails with ENOSYS, then all subsequent
operations on that path cascade-fail with ENOENT. Accounts for ~90% of
individual test failures in this group.

Affected: chmod/00,01,11 | link/00,01,10 | mkdir/00,01,10 |
open/00,01,06,17,22,24 | rename/00,09,10,12,13,14,20,23,24 |
rmdir/01,06 | symlink/08 | unlink/00,11 | utimensat/00

#### 2. No atime tracking

utimensat sets atime to the mtime value instead of the provided atime.
The FUSE layer stores a single mtime per path; atime is always aliased
to mtime.

Affected: utimensat/00 (tests 4, 9), utimensat/02, 04, 05, 08, 09

#### 3. Permission enforcement (-u 65534 tests)

Some tests run operations as the `nobody` user (uid 65534) and expect
EACCES/EPERM. We use `DefaultPermissions` which delegates to the kernel,
but some edge cases differ.

Affected: ftruncate/00,05,06 | truncate/00,05,06 | open/22 | rename/20

#### 4. No open-unlink semantics

After `open()` + `unlink()`, POSIX requires the file descriptor to remain
valid (fstat returns nlink=0, read still works). Our FUSE layer removes
the inode immediately on unlink.

Affected: unlink/14 (tests 4, 6)

#### 5. Symlink target length limit

Creating a symlink with a 255-byte target returns EIO. The archive's
path_size may be too small for the target, or safename validation
rejects it.

Affected: symlink/02 (test 6)

#### 6. Description-less test failures

Some tests fail without printing what they tested (no "tried X, expected
Y, got Z" output). Parent directory mtime updates (commit `82a0d79`)
fixed many of these, but some remain.

Affected: various tests across known-fail files (67 remaining)

#### 7. Directory-over-directory rename

Renaming a directory over an existing empty directory should succeed
(POSIX). Currently returns EEXIST instead.

Affected: rename/09 (tests 2259, 2279, 2299, 2311 and cascading)

### UNSUPPORTED Group (51 files, 1818 tests)

Features we intentionally do not implement:

| Feature | Reason |
|---------|--------|
| chown | Ownership is transient (session-only), not POSIX-persistent |
| mkfifo | FIFOs not supported in archive format (ENOSYS) |
| mknod | Device nodes not supported in archive format (ENOSYS) |
| chflags | BSD-specific, skipped on Linux |
| posix_fallocate | Not implemented |

## Actionable Improvements

Roughly ordered by impact (test count that would move to PASS):

1. **atime tracking** -- Store atime separately from mtime. Would fix
   utimensat/00,02,04,05,08,09 (~6 files, ~32 tests).

2. **Open-unlink semantics** -- Keep inode alive while file handles are
   open. Would fix unlink/14 (2 tests) and potentially others.

3. **Directory-over-directory rename** -- Allow renaming a directory
   over an existing empty directory. Would fix rename/09 subset.

4. **Symlink target length** -- Investigate the 255-byte target failure
   in symlink/02. May need path_size increase or safename adjustment.

## Completed Improvements

- **ctime/mtime on parent after mutations** -- Implemented in `82a0d79`.
  Parent directory mtime is now updated on create/remove/rename.

- **Rename metadata preservation** -- Implemented in `82a0d79`.
  Modified modes, times, uids, gids, and nlink are now properly
  transferred during rename and cleaned up on overwrite.

- **Symlink type preservation in rename** -- Implemented in `82a0d79`.
  Renamed symlinks retain their type in dir_contents and archive.

- **Symlink ENAMETOOLONG mapping** -- Implemented in `82a0d79`.
  UnsafeFilename/PathTooLong errors now return ENAMETOOLONG instead
  of EIO.
