# pjdfstest Results

Last run: **2025-02-15** (commit `cfd7b44`)

## How to Run

```bash
cargo build --release
sudo tests/bin/run-pjdfstest.sh
```

Requires the pjdfstest suite at `~/projects/ref/pjdfstest/`.
Results are written to `~/projects/pjdfstest-results/`.

## Test Groups

The script splits pjdfstest into three groups:

| Group | Files | Tests | Result |
|-------|------:|------:|--------|
| **PASS** (should all pass) | 134 | ~4010 | PASS |
| **KNOWN FAILURES** (limitations) | 45 | ~2860 | FAIL |
| **UNSUPPORTED** (not implemented) | 51 | 1818 | FAIL |

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
Y, got Z" output). Likely testing ctime/mtime updates on parent
directories after operations.

Affected: symlink/00 (tests 11-12) | rmdir/00 (tests 8-9)

#### 7. Stale inode after rename

After renaming a file, lstat on the old path sometimes returns an inode
number instead of ENOENT, suggesting the directory table retains a ghost
entry.

Affected: rename/10 (later tests, e.g. 413, 415, 421, 423)

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

2. **ctime/mtime on parent after mutations** -- Update parent directory
   ctime/mtime when creating/removing children. Would likely fix the
   description-less failures in symlink/00, rmdir/00 and possibly others.

3. **Open-unlink semantics** -- Keep inode alive while file handles are
   open. Would fix unlink/14 (2 tests) and potentially others.

4. **Stale inode after rename** -- Audit rename_entry to ensure old
   directory entries are fully removed. Would fix rename/10 subset.

5. **Symlink target length** -- Investigate the 255-byte target failure
   in symlink/02. May need path_size increase or safename adjustment.
