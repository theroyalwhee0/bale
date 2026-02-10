# Bale Archive Format Specification

Version: 1.0.0

## Overview

Bale is a mmap-first, zero-copy archive format designed for efficient random
access to application-level asset packages (e.g., game assets, resource
bundles). It uses fixed-stride tables for O(1) entry lookup and
alignment-padded data blocks for direct memory mapping.

Bale does not store ownership (uid/gid), extended attributes, or ACLs. The
mode field preserves file type and permission bits for portability but is not
intended for full filesystem round-trip fidelity.

A bale archive consists of five sections, laid out sequentially:

1. **File Header** — Format identification and version (8 bytes)
2. **Data Blocks** — File contents, each aligned to a boundary
3. **Entry Table** — Fixed-stride metadata for each entry (no paths)
4. **Directory Table** — Fixed-stride path-to-entry-ID mapping
5. **Trailer** — Configuration, table offsets, counts, and validation (64 bytes)

All multi-byte integer fields are **little-endian**. The format does not
include an endianness indicator; little-endian encoding is a fixed property
of version 1.x.x. A future major version may introduce endianness
negotiation if needed.

All multi-byte fields in the entry row and trailer are naturally aligned
(u64 at 8-byte boundaries, u32 at 4-byte boundaries, u16 at 2-byte
boundaries). This permits direct struct mapping on little-endian platforms
without unaligned access concerns.

## Archive Layout

```text
┌──────────────────────────────────────┐  Offset 0
│ File Header (8 bytes)                │
├──────────────────────────────────────┤
│ Alignment Padding (zeros)            │
├──────────────────────────────────────┤  Aligned
│ Data Block [0]                       │
│   Data (block_size bytes)            │
│   Padding to alignment (zeros)       │
├──────────────────────────────────────┤  Aligned
│ Data Block [1]                       │
│   ...                                │
├──────────────────────────────────────┤
│ ...                                  │
├──────────────────────────────────────┤
│ Slack Space (optional, zeros)        │
├──────────────────────────────────────┤
│ Entry Table                          │
│   Entry Row [0] (64 bytes)           │
│   Entry Row [1] (64 bytes)           │
│   ...                                │
├──────────────────────────────────────┤
│ Slack Space (optional, zeros)        │
├──────────────────────────────────────┤
│ Directory Table                      │
│   Directory Row [0] (path_size + 4)  │
│   Directory Row [1] (path_size + 4)  │
│   ...                                │
├──────────────────────────────────────┤
│ Slack Space (optional, zeros)        │
├──────────────────────────────────────┤
│ Trailer (64 bytes)                   │  Last 64 bytes of file
└──────────────────────────────────────┘
```

## Concurrency

Bale archives support concurrent readers. Multiple processes may mmap and
read the same archive simultaneously. Write operations (insertion, deletion,
unlinking, compaction) require exclusive access. The format does not provide
internal locking; writers are responsible for coordinating exclusive access
through external mechanisms (e.g., file locks, single-writer architecture).

All write operations — including insertion, deletion, unlinking, compaction,
rename, and content replacement — require the writer to hold an exclusive
lock on the archive file for the duration of the operation. Readers must not
hold shared locks that would block writers indefinitely. The specific locking
mechanism is outside the scope of this format; implementations may use
`flock`, `fcntl` advisory locks, or application-level coordination.

### Mutation Modes

The format supports two mutation modes:

**Atomic replace:** The writer constructs a new archive in a temporary file
and atomically renames it over the original. This provides crash consistency:
readers see either the old or the new archive, never a partial state.
Compaction always uses this mode (see Compaction).

**In-place mutation:** The writer modifies the existing archive file
directly. This mode is appropriate for workloads where frequent small
mutations make full-archive rewrites impractical (e.g., FUSE-mounted
archives used for interactive editing). In-place mutation is not crash-safe
— a crash during a write may leave the archive in an unrecoverable state.
Writers that need crash consistency should use atomic replace mode. Writers
that use in-place mutation should ensure the archive contents are rebuildable
from external sources.

Readers that have the old file mmap'd continue to read from the old inode
after an atomic rename; they are unaffected by the rename. However, any
entry IDs cached from the old archive are invalid in the new one (see
Compaction).

## Version Compatibility

Bale uses semantic versioning for the format version:

- **Major version**: A reader must reject archives with a different major
  version than it supports. Major version changes indicate breaking
  structural changes.
- **Minor version**: A reader may open archives with a minor version higher
  than it understands. New minor versions may assign meaning to reserved
  bytes; readers that do not understand those fields ignore them (see
  Reserved Fields). The archive remains structurally compatible.
- **Patch version**: Always compatible. Patch version changes reflect
  editorial or clarification changes to the specification with no
  structural differences.

A 1.0.0 reader can safely open a 1.x.x archive. A 1.0.0 reader must
reject a 2.x.x archive.

The file header is the sole location of the format version. All version
checks reference the file header at offset 0. Future major versions may
reorganize the archive layout; the file header tells the reader which
parser to use.

## Configuration

Two parameters control the archive geometry:

| Setting   | Default      | Range              | Description                     |
|-----------|--------------|--------------------|---------------------------------|
| Alignment | 4096 (2^12)  | 2^0 to 2^16       | Data block alignment in bytes   |
| Path size | 256          | 1 to 4096          | Maximum path length in bytes    |

**Alignment** determines data block padding. Stored as a power-of-two exponent
(e.g., 12 means 2^12 = 4096 bytes).

Archives containing many small files benefit from a lower alignment value.
With the default alignment of 4096, each file's data is padded to a
4096-byte boundary, which can waste significant space when files are small.
Setting `alignment_power=0` (alignment=1) eliminates all padding, at the
cost of losing page-aligned mmap access. When `alignment_power=0`, data
blocks are packed contiguously with no gap between the file header and the
first data block (i.e., the first data block starts at offset 8). Choose
alignment based on the expected access pattern: page-aligned (4096+) for
mmap random access, low or none (1) for space efficiency with many small
files.

Alignment applies only to data blocks. The entry table, directory table,
and trailer are not required to be aligned.

A reader must reject an archive with an `alignment_power` value greater
than 16.

**Path size** is the fixed width of the path field in the directory table. Paths
shorter than `path_size` are null-padded. Paths longer than `path_size` cannot
be stored. Users should choose `path_size` based on their longest expected
path. Oversizing wastes `(path_size - avg_path_length) * entry_count` bytes
in the directory table.

A reader must reject archives with a `path_size` value outside the range
1–4096.

## Capacity Limits

The maximum archive file size is bounded by `u64` (the `archive_size` field
and all byte offset fields are 64-bit unsigned integers). The practical
maximum archive size is platform-specific; implementations that use mmap
may be limited by the platform's address space. Handling of archives that
exceed the platform's mmap capacity (e.g., windowed mapping or read/seek
fallback) is an implementation concern.

The maximum number of entries is bounded by `u32` (the `entry_count` and
`next_id` fields are 32-bit unsigned integers). Since entry ID `0` is
reserved as a tombstone, the theoretical maximum number of live entries in
an archive is 2^32 − 1.

## Reserved Fields

Throughout the format, several fields are marked as reserved. Writers must
set all reserved bytes to zero. Readers must ignore the contents of reserved
bytes and must not reject an archive solely because reserved bytes are
non-zero. This allows future minor versions to assign meaning to reserved
bytes without breaking older readers.

## Reserved Paths

A path component is reserved if it matches the regular expression:

```text
^\.bale(\..+)?$
```

That is, a component equal to `.bale` or starting with `.bale.` followed by
one or more characters.

Archives must not contain entries with reserved path components. This applies
to any path component (segment between `/` separators), not just the first.
Writers must reject insertion of paths containing reserved components.
Readers should treat the presence of reserved path components as corruption
in strict validation mode.

**Reserved:** `.bale`, `.bale.txt`, `.bale.index`, `foo/.bale/bar`,
`foo/.bale.metadata`

**Not reserved:** `.balefire`, `.bales`, `bale`, `foo.bale`, `mybale/file.txt`

## File Header

8 bytes at offset 0. Identifies the file as a bale archive and declares
the format version. The file header is the sole source of truth for format
identification and version.

| Offset | Size | Field         | Description                            |
|--------|------|---------------|----------------------------------------|
| 0      | 4    | Magic         | `BALE` (0x42 0x41 0x4C 0x45)          |
| 4      | 1    | Separator     | 0x00                                   |
| 5      | 1    | Major version | Format major version                   |
| 6      | 1    | Minor version | Format minor version                   |
| 7      | 1    | Patch version | Format patch version                   |

The magic bytes allow `file(1)` and similar tools to identify bale archives
by reading the first 5 bytes. The separator byte separates the magic from
the version fields. The version bytes enable readers to select the
appropriate parser for the format version.

## Data Blocks

One data block per entry that has content (regular files and symlinks).
Directories and empty files have no data block.

Each data block starts at an alignment boundary. The first data block begins
at the first alignment boundary at or after byte 8 (the end of the file
header). With the default alignment of 4096, the first data block starts at
offset 4096. With `alignment_power=0` (alignment=1), the first data block
starts immediately at offset 8.

All alignment padding bytes — between the file header and the first data
block, and between each data block's content and the next alignment
boundary — must be zero. Readers are not required to validate padding
content during normal operation.

Data blocks contain only the stored data bytes — there is no per-block
header. All metadata (size, offset, checksum, compression) is stored in
the entry table.

### Data Block Layout

```text
┌────────────────────────────────────────┐  Aligned offset
│ Data (block_size bytes)                │
├────────────────────────────────────────┤
│ Padding (zeros to next alignment)      │
└────────────────────────────────────────┘
```

**Total block size:** `align_up(block_size, alignment)`

### Data Integrity

Each entry row contains a CRC-32C covering the stored bytes (post-compression)
of that entry's data block. This allows per-file integrity validation on
access. When compression is used, the compression format's own integrity
mechanisms are expected to cover decompression correctness.

When `block_size` is 0 (empty files, directories), the `crc32c` field must
be set to `0x00000000`. Readers must skip CRC validation for entries with
`block_size = 0`.

Data block corruption is detected only when an individual entry's data is
read and its CRC-32C is validated. The metadata CRC-32C in the trailer (see
Trailer) covers structural integrity of the archive's tables and
configuration but intentionally does not cover data blocks, as including
them would require reading the entire archive on open.

### Dead Data Blocks

Data blocks may become unreferenced through two mechanisms: entry tombstoning
(see Deletion) and content replacement (see Content Replacement). In both
cases the orphaned data block remains in the archive as dead space until
compaction reclaims it (see Compaction).

Writers may overwrite orphaned data blocks with zeros, but this is not
required. For large data blocks, the I/O cost of zeroing may outweigh any
benefit.

Unreferenced regions between live data blocks are not corruption. Validation
tools should not flag dead space between the file header and the entry table
as an error.

### CRC-32 Algorithm

All CRC-32 values in this format use the CRC-32C (Castagnoli) algorithm:
polynomial 0x1EDC6F41, reflected, initial value 0xFFFFFFFF, final XOR
0xFFFFFFFF. This applies to both the per-entry data CRC-32C and the metadata
CRC-32C in the trailer. CRC-32C values are stored as 32-bit little-endian
unsigned integers, consistent with all other multi-byte integer fields in
the format.

### Symlink Data

Symlinks store their target path as the data block content. The target path
is stored as raw UTF-8 bytes without a null terminator. The `file_size` and
`block_size` equal the byte length of the target path.

Symlink targets must be relative paths. A target must not begin with `/`.
The resolved target, after applying any `.` and `..` components relative to
the symlink's parent directory, must not resolve to a path above the archive
root. Writers must reject symlinks whose resolved targets escape the archive
root. Symlink targets are not required to reference an existing entry;
dangling symlinks are valid.

### Symlink Resolution

The format defines two levels of symlink handling:

**Low-level (direct lookup):** Path lookup is an exact string match in the
directory table. When a lookup returns a symlink entry, the caller receives
the symlink entry itself. No resolution is performed. This is appropriate
for low-level APIs that treat symlinks as opaque data entries.

**High-level (path-walking resolution):** Higher-level interfaces (e.g.,
FUSE mounts, filesystem-like APIs) that need transparent symlink traversal
must implement path-walking resolution. The algorithm is:

1. **Fast path:** Look up the full path in the directory table. If found
   and the entry is not a symlink (or the caller wants the symlink itself),
   return the entry. This covers the common case with a single lookup.
2. **Component walk:** If the full path is not found, decompose the path
   into components and resolve front-to-back:
   a. Look up the first component in the directory table.
   b. If it is a symlink, resolve the symlink target (join with the
      symlink's parent directory, normalize `.` and `..` components),
      prepend the resolved path to the remaining components, and restart
      from step 1 with the reconstructed full path.
   c. If it is a directory, advance to the next component: look up the
      path formed by all components resolved so far plus the next component.
   d. If it is a regular file and there are remaining components, the path
      is invalid. Return an error (the equivalent of ENOTDIR — an
      intermediate path component is not a directory or symlink).
   e. If not found and there are remaining components, the path does not
      exist. Return "not found."
   f. Continue until all components are resolved.
3. **Depth limit:** Track the total number of symlink resolutions across
   the entire walk. If the count exceeds 256, return an error. This serves
   as the cycle-breaking mechanism.

Symlink resolution is confined to the archive. A resolved path that would
escape the archive root (after normalizing `..` components) is invalid; the
writer must reject such symlinks at insertion time, and the reader must
treat such a path as an error if encountered.

Implementations may cache intermediate resolution results to avoid redundant
lookups when resolving deeply nested paths with multiple symlinks.

### Slack Space

Between archive sections (after the last data block, after the entry table,
after the directory table), a writer may leave zero-filled slack space. This
space reserves room for future insertions or table growth without requiring
all subsequent sections to be rewritten.

Slack space must be zero-filled. It has no header or metadata of its own;
its presence is implied by gaps between section boundaries. The gap between
the end of the last data block (computable from entry table metadata) and
`entry_table_offset`, between the end of the entry table and
`directory_table_offset`, and between the end of the directory table and the
trailer, may all contain slack space.

Readers must not treat gaps between sections as corruption.

When inserting a new entry into a non-compacted archive, a writer may place
the new data block in the data slack space if sufficient room exists. If
sufficient slack exists after the entry table and directory table, the writer
may append new rows to the tables in place and rewrite only the trailer.
If insufficient slack space exists in any section, the writer must rewrite
that section and all subsequent sections, optionally allocating new slack
space.

Compaction removes all slack space. Writers that anticipate future
insertions into a non-compacted archive may pre-allocate slack space
when creating or rewriting the archive. The amount of slack space to
pre-allocate is an implementation decision based on expected workload;
the format imposes no constraints on slack space sizing.

## Entry Table

A contiguous array of fixed-stride rows, one per entry. Contains metadata
but no paths. The entry table offset and count are stored in the trailer.

The entry table is always ordered by entry ID. Each entry ID `k` is stored
at row index `k - 1`, providing O(1) direct index lookup. This invariant
is maintained across all operations including deletion (see Deletion).
After compaction, the entry table is dense with no tombstoned rows, and
`entry_count` equals the number of live entries.

When `entry_count` is 0, the `entry_table_offset` must be 0, indicating
no entry table is present.

### Entry Row

64 bytes per entry. All multi-byte fields are naturally aligned.

| Offset | Size | Field             | Description                              |
|--------|------|-------------------|------------------------------------------|
| 0      | 4    | Entry ID          | LE, u32, unique within archive (0 = tombstone) |
| 4      | 4    | CRC-32C           | LE, u32, checksum of stored data bytes   |
| 8      | 8    | Data offset       | LE, u64, byte offset to data (0 = none)  |
| 16     | 8    | File size         | LE, u64, original uncompressed size      |
| 24     | 8    | Block size        | LE, u64, stored size (after compression) |
| 32     | 8    | Created time      | LE, i64, Unix epoch milliseconds         |
| 40     | 8    | Modified time     | LE, i64, Unix epoch milliseconds         |
| 48     | 4    | Mode              | LE, u32, Unix permissions and file type  |
| 52     | 1    | Compression       | 0 = none (stored)                        |
| 53     | 1    | Flags             | u8, bitfield (see Entry Flags)           |
| 54     | 10   | Reserved          | Must be zero (see Reserved Fields)       |

**Stride:** 64 bytes (fixed for all entries).

**Entry count** in the trailer is the physical row count of the entry table,
including tombstoned rows. This allows readers to compute the table's byte
size as `entry_count * 64`.

**Distinguishing empty files from directories:** Both regular files with no
content and directories have `data_offset = 0`, `file_size = 0`, and
`block_size = 0`. They are distinguished solely by the file type bits in
the `mode` field (0x8 for regular file, 0x4 for directory).

### Entry Flags

The `flags` field is an 8-bit bitfield.

| Bit  | Mask | Name     | Description |
|------|------|----------|-------------|
| 0–7  | —    | Reserved | Must be zero |

In version 1.0.0, all 8 bits are reserved and must be zero. Future minor
versions may define flag bits; readers that do not understand a flag bit
must ignore it (see Reserved Fields).

### Timestamps

The `created_time` and `modified_time` fields on all entry types reflect the
values from the source filesystem at the time the entry was added to the
archive. Archive operations (insertion, deletion, unlinking, compaction,
rename) do not update timestamps on existing entries. If an entry's content
is replaced, the writer should update `modified_time` to reflect the new
content's source timestamp.

### Deletion

Two operations remove entries from an archive:

**Unlink** removes a single directory row by tombstoning it (path zeroed,
entry ID set to 0). Unlink does not follow symbolic links — unlinking a
symlink path tombstones the symlink's directory row, not the target's. If
no live directory rows reference the entry ID after the unlink, the entry
row is also tombstoned (entry ID set to 0, all other fields zeroed). The
orphaned data block remains as dead space until compaction (see Dead Data
Blocks). If other directory rows still reference the entry ID, the entry
row remains live.

**Delete** removes an entry row by tombstoning it (entry ID set to 0, all
other fields zeroed) and tombstones all directory rows that reference that
entry ID. The orphaned data block remains as dead space until compaction
(see Dead Data Blocks).

Both operations must clear the compacted flag (trailer flags bit 0).

Tombstoned rows are reclaimed during compaction (see Compaction).

### Compaction

Compaction produces a new archive containing only the live entries of the
source archive. It is equivalent to writing a new archive from the live
entries: all live entries are renumbered sequentially starting from 1,
tombstoned rows are removed from both tables, the directory table is fully
sorted, the compacted flag (trailer flags bit 0) is set, `next_id` is
reset to N+1, and all slack space is removed. All unreferenced data blocks
are reclaimed, including those orphaned by entry tombstoning and content
replacement.

Compaction invalidates all previously-issued entry IDs. Any external
system holding entry ID references (caches, mount points, inode maps) must
be rebuilt after compaction.

Entry IDs are not stable across compaction. External systems must not
persist entry IDs as long-lived references. After compaction, all entry ID
mappings from prior to the compaction are invalid and must be discarded.
This is inherent to the design: entry IDs are session-scoped identifiers
for efficient lookup, not durable handles.

Compaction always produces a new file. Writers should write the compacted
archive to a temporary file and atomically rename it over the original
(see Mutation Modes). Readers that have the old file mmap'd continue
reading the old data via the original inode. New readers opening the file
after the rename see the compacted archive.

### Compression

In version 1.0.0, the only valid compression value is `0` (stored /
uncompressed). When compression is not used, `file_size` and `block_size`
are equal. Future versions will define additional compression methods.
Values 1–255 are reserved.

### Entry IDs

- Entry IDs are unsigned 32-bit integers.
- ID 0 is reserved as a tombstone/sentinel and must not be assigned to a
  live entry. Entry ID 0 never has a row in the entry table; row index 0
  always corresponds to entry ID 1.
- IDs are assigned sequentially starting from 1.
- IDs are never reused within a session. The next available ID is tracked
  in the trailer's `next_id` field.
- New entries are always appended to the end of the entry table. Tombstoned
  rows are never reused for new entries; they remain as placeholders to
  preserve the entry ID to row index mapping. Only compaction reclaims
  tombstoned rows.
- If `next_id` would exceed `u32::MAX` (4,294,967,295), the archive is
  full. The value `u32::MAX` itself is a valid entry ID; the overflow
  condition is that no value *after* `u32::MAX` can be represented.
  Writers must reject further insertions when `next_id` would overflow.
  Compaction reclaims tombstoned IDs and resets `next_id` to N+1, restoring
  the ability to insert new entries.
- Writers should monitor `next_id` and trigger compaction before the ID
  space is exhausted.

### File Types and Mode

The `mode` field uses standard Unix mode encoding:

| Bits    | Mask     | Description      |
|---------|----------|------------------|
| 15–12   | 0xF000   | File type        |
| 11–9    | 0x0E00   | Special bits     |
| 8–0     | 0x01FF   | Permission bits  |

**File type** (bits 15–12) must be one of the following values. Any other
value is invalid and the reader must reject the entry.

| Value  | Type             |
|--------|------------------|
| 0x8    | Regular file     |
| 0x4    | Directory        |
| 0xA    | Symbolic link    |

**Special bits** (bits 11–9) must be zero. Setuid, setgid, and sticky bits
are not supported.

**Permission bits** (bits 8–0) are meaningful for regular files and
directories. For symbolic links, permission bits are fixed to `0o777`
(symlink mode is always `0o120777`).

### Directories

Directories have `data_offset = 0`, `file_size = 0`, and `block_size = 0`.
The directory's existence is recorded in both the entry table (with mode
indicating directory type) and the directory table (path mapped to entry ID).

Hard links to directories are not permitted. Each directory entry ID must be
referenced by exactly one live directory row. Writers must reject any
operation that would create a second live directory row referencing an entry
with directory file type.

### Empty Files

Regular files with no content have `data_offset = 0`, `file_size = 0`, and
`block_size = 0`. They are distinguished from directories by their mode
(see Entry Row).

### Symbolic Links

Symlink entries always have mode `0o120777`. Permission bits on symlinks are
not meaningful and are normalized to `0o777` on write.

### Count Consistency

The `entry_count` and `directory_entry_count` fields in the trailer are
independent values. They may differ due to hard links (multiple directory
rows referencing one entry), tombstoned rows, or other valid archive states.
A reader must not treat a mismatch between these counts as corruption.

However, certain combinations indicate corruption:

- A live directory row (entry ID ≠ 0) whose entry ID exceeds `entry_count`
  references a nonexistent entry row. Readers should treat this as
  corruption.
- A live entry row with no live directory row referencing it is unreachable
  by path lookup. This is not corruption but represents wasted space;
  compaction removes unreachable entries.

These checks provide a fast structural validation without requiring a full
walk of both tables.

## Directory Table

A contiguous array of fixed-stride rows mapping paths to entry IDs. The
directory table offset and count are stored in the trailer.

When `directory_entry_count` is 0, the `directory_table_offset` must be 0,
indicating no directory table is present.

### Directory Row

`path_size + 4` bytes per row.

| Offset      | Size        | Field    | Description                    |
|-------------|-------------|----------|--------------------------------|
| 0           | `path_size` | Path     | UTF-8, null-padded             |
| `path_size` | 4           | Entry ID | LE, references an entry row    |

**Stride:** `path_size + 4` bytes (fixed for all rows).

**Ordering:** When the compacted flag (trailer flags bit 0) is set, all rows
are live and sorted in lexicographic unsigned byte order, enabling binary
search for path lookups and prefix-based directory listing. Lexicographic
unsigned byte order is defined as unsigned byte comparison over the full
`path_size` bytes of each path field. When the compacted flag is clear, the
table may contain tombstoned rows (entry ID 0) at any position and live rows
may not be in sorted order. Readers must use linear scan or another lookup
strategy that does not depend on sort order, skipping tombstoned rows during
all search and scan operations.

**Implementation note on binary search:** Because paths are null-padded to
the full `path_size`, binary search must compare the full `path_size` bytes,
not just up to the first null byte. Null bytes (0x00) sort before any valid
UTF-8 byte, so shorter paths sort before longer paths that share the same
prefix (e.g., `a\0\0...` < `ab\0...` < `b\0\0...`).

**Directory entry count** in the trailer is the physical row count of the
directory table, including tombstoned rows. This allows readers to compute
the table's byte size as `directory_entry_count * (path_size + 4)`.

`entry_count` and `directory_entry_count` are independent values. They may
differ due to hard links (multiple directory rows referencing one entry),
tombstoned rows, or other valid archive states. A reader must not treat a
mismatch between these counts as corruption.

### Tombstoned Directory Rows

A tombstoned directory row has its path set to all null bytes and its entry
ID set to 0. Readers must skip rows with entry ID 0 during all search and
scan operations.

A directory row with a non-zero path but entry ID 0 is malformed. Readers
should treat this as a tombstoned row (skip it) during normal operation.
Strict validation tools should flag this condition as a warning.

### Duplicate Paths

Each live path in the directory table must be unique. Writers must reject
an insertion that would create a second live directory row with the same
path. Readers that encounter duplicate live paths should treat the archive
as corrupt.

### Deletion

Directory rows are tombstoned by setting the path to all null bytes and the
entry ID to 0. The row remains in place, preserving the fixed-stride layout.
See Entry Table — Deletion for the unlink and delete operations. Readers
must skip rows with entry ID 0 during all search and scan operations.

### Paths

- Paths are UTF-8 encoded.
- Paths must not contain embedded null bytes (0x00). Null bytes appear only
  as padding after the path content.
- Paths use `/` as the directory separator.
- Paths are relative (no leading `/`).
- Paths must not contain `.` or `..` components.
- Paths must not have trailing `/` separators.
- Paths shorter than `path_size` are padded with null bytes (`0x00`).
- Directory entries include paths for the directory itself (e.g., `src/`
  is stored as `src`).
- Paths must not contain reserved path components (see Reserved Paths).

### Unicode Normalization

The format does not require or perform Unicode normalization. Paths are
compared and stored as raw UTF-8 byte sequences. Two paths that are
visually identical but use different Unicode representations (e.g., NFC
vs. NFD) are treated as distinct paths. Writers are responsible for
normalizing paths before insertion if normalization is desired for their
use case.

### Root Directory

The root directory has no entry in the entry table or directory table. To
list top-level contents, scan the directory table for live rows (entry
ID ≠ 0) whose path contains no `/` separator. This is always a linear scan
regardless of the compacted flag.

### Directory Listing

To list a directory's contents when the compacted flag is set, binary search
for the first path with the byte prefix `dir_name/` (the trailing `/` is
required to avoid matching unrelated paths such as `dir_name2/...`), then
scan forward while the prefix matches, skipping any tombstoned rows (entry
ID 0). When the compacted flag is clear, scan the entire directory table for
paths matching the prefix, skipping tombstoned rows. In either case, direct
children are entries where the remainder after the prefix contains no `/`
separator. This yields a single-level directory listing without scanning the
full table (when compacted).

Note that the prefix search `dir_name/` does not match the directory entry
itself (stored as `dir_name` without a trailing slash). To check whether a
directory exists, look up the path `dir_name` directly in the directory table.
These are two distinct operations: existence check (exact path lookup) and
content listing (prefix scan).

Directory listing operates on the directory table directly and does not
resolve symlinks. A symlink entry appearing in a listing is returned as-is;
callers that need transparent symlink traversal must resolve symlinks at a
higher level (see Symlink Resolution).

### Hard Links

Multiple directory rows may reference the same entry ID with different paths.
This models hard links: several paths sharing the same file content and
metadata.

All paths sharing an entry ID share all metadata, including timestamps,
mode, and content. A change made through any path (e.g., content
replacement) is visible through all paths referencing the same entry ID.
This mirrors standard Unix hard link semantics.

Hard links are only permitted for regular files and symbolic links.
Directories must not be hard-linked (see Directories under Entry Table).

## Trailer

64 bytes, always the last 64 bytes of the archive. Contains all archive
configuration, table offsets, and counts needed to read the archive. All
multi-byte fields are naturally aligned.

| Offset | Size | Field                  | Description                              |
|--------|------|------------------------|------------------------------------------|
| 0      | 8    | Entry table offset     | LE, u64, byte offset to entry table (0 = no table) |
| 8      | 8    | Directory table offset | LE, u64, byte offset to directory table (0 = no table) |
| 16     | 8    | Archive size           | LE, u64, expected total file size in bytes |
| 24     | 4    | Entry count            | LE, u32, number of entry rows (including tombstones) |
| 28     | 4    | Directory entry count  | LE, u32, number of directory rows (including tombstones) |
| 32     | 4    | Next entry ID          | LE, u32, next ID to assign               |
| 36     | 2    | Path size              | LE, u16, maximum path length (1–4096)    |
| 38     | 1    | Alignment power        | u8, exponent N where alignment = 2^N (0–16) |
| 39     | 1    | Flags                  | u8, bitfield (see Trailer Flags)         |
| 40     | 16   | Reserved               | Must be zero (see Reserved Fields)       |
| 56     | 4    | Magic                  | `BALE` (0x42 0x41 0x4C 0x45)            |
| 60     | 4    | Metadata CRC-32C       | CRC-32C over file header + entry table + directory table + trailer bytes 0–59 |

The last 8 bytes of the archive are the magic and CRC-32C. Tools can
identify a bale archive by checking the last 8 bytes for the `BALE` magic
at offset -8 from end of file.

### Trailer Validation

The trailer provides three layers of validation, in order of cost:

1. **Magic** (`BALE` at bytes 56–59): Fast rejection of non-bale data.
2. **Archive size** (bytes 16–23): Confirms the file has not been truncated
   or appended to. Compare against actual file size; reject on mismatch.
3. **Metadata CRC-32C** (bytes 60–63): Validates structural integrity of
   the archive's header, tables, and configuration.

Readers should validate the magic and archive size. Validation of the
metadata CRC-32C is strongly recommended on first open or after any
modification, but is not mandatory. If the CRC is validated and does not
match, the archive should be treated as corrupt.

### Trailer Flags

The `flags` field is an 8-bit bitfield.

| Bit  | Mask | Name      | Description                                       |
|------|------|-----------|---------------------------------------------------|
| 0    | 0x01 | Compacted | Directory table is fully sorted with no tombstones |
| 1–7  | —    | Reserved  | Must be zero                                      |

**Compacted (bit 0):** When set, the directory table contains no tombstoned
rows and all live rows are sorted in lexicographic unsigned byte order.
Readers may use binary search for path lookup. When clear, the directory
table may contain tombstoned rows at any position and live rows may not be
in sorted order; readers must use linear scan or another strategy that does
not depend on sort order.

An empty archive (zero directory rows) should set the compacted flag, as
the empty table trivially satisfies the sorted, no-tombstones invariant.

A compaction operation must set this bit. Any insertion or deletion that does
not fully re-sort the directory table and remove all tombstones must clear
this bit.

### Metadata CRC-32C

The metadata CRC-32C is computed over the logical concatenation of four
non-contiguous regions of the archive file, fed into the CRC state machine
in the following order:

1. The file header (8 bytes at offset 0)
2. The entire entry table (all bytes)
3. The entire directory table (all bytes)
4. Trailer bytes 0–59 (all trailer fields except the CRC itself, including
   the magic at bytes 56–59)

During computation, the CRC field (trailer bytes 60–63) is treated as four
zero bytes. When the entry table or directory table is empty (offset is 0,
count is 0), the corresponding region contributes zero bytes to the CRC
computation.

These regions are not contiguous on disk — data blocks and optional slack
space separate the file header from the tables. The CRC is computed by
feeding these four regions into the CRC algorithm sequentially, not by
reading a single contiguous byte range.

Note that the `BALE` magic appears in both the file header and the trailer;
both occurrences are included in the CRC-32C computation.

This single checksum validates the identification, version, and structural
integrity of the archive. If the checksum is validated and does not match,
the archive should be treated as corrupt.

The per-entry CRC-32C in each entry row provides separate data integrity
validation on a per-file basis (see Data Integrity).

### Archive Size

The `archive_size` field records the expected total size of the archive file
in bytes. On open, a reader should compare this value against the actual
file size. A mismatch indicates file truncation or corruption.

The trailer is always located at the last 64 bytes of the file
(`file_size - 64`). Because `archive_size` must equal the actual file size,
any external modification that changes the file size (appending data,
truncation) will cause `archive_size` validation to fail. Tools that
manipulate the file must preserve or update the trailer accordingly.

### Streaming Writes

The archive format is not streamable to non-seekable outputs in a single
pass. The trailer contains offsets and counts that depend on the data blocks,
entry table, and directory table having already been written, and the
metadata CRC-32C covers all of these sections. Writers must either buffer
the archive in memory, write to a seekable output, or use a two-pass
strategy.

## Archive Mutation Operations

In addition to deletion, unlinking, and compaction (described in Entry Table
— Deletion and Compaction), the following mutation operations are defined.
All mutation operations require exclusive access to the archive (see
Concurrency).

### Insertion

To insert a new entry into an existing archive:

1. Assign the next available entry ID from `next_id` in the trailer.
   Reject if `next_id` would overflow `u32`.
2. Write the new data block. If sufficient slack space exists between the
   last data block and the entry table, the data block may be placed there,
   consuming data slack space. Otherwise, the data block is written after
   the existing data blocks and subsequent sections are rewritten at new
   offsets.
3. Append a new entry row to the end of the entry table. If sufficient
   slack space exists after the entry table, the row may be written in
   place. Otherwise, the entry table and all subsequent sections must be
   rewritten.
4. Append a new directory row to the end of the directory table. If
   sufficient slack space exists after the directory table, the row may
   be written in place. Otherwise, the directory table and trailer must
   be rewritten.
5. Increment `next_id`, update `entry_count` and `directory_entry_count`,
   clear the compacted flag, recompute `archive_size` and the metadata
   CRC-32C, and rewrite the trailer.

### Hard Link Creation

To create a hard link (a new path referencing an existing entry):

1. Locate the target entry by its existing path or entry ID. Verify the
   target entry is a regular file or symbolic link. Hard links to
   directories are not permitted (see Directories under Entry Table).
2. Verify no live directory row exists for the new path.
3. Append a new directory row to the directory table with the new path and
   the target's entry ID. No new entry row is created.
4. Increment `directory_entry_count`, clear the compacted flag, recompute
   `archive_size` and the metadata CRC-32C, and rewrite the trailer.

### Rename

To rename an entry (change its path without modifying its content or
metadata):

1. Locate the directory row for the source path.
2. Verify no live directory row exists for the destination path.
3. Write the destination path into the directory row (or tombstone the old
   row and append a new row, depending on implementation strategy).
4. If the entry is a directory, update all descendant paths: scan the
   directory table for all live rows whose path starts with `old_path/`
   and replace the prefix with `new_path/`. If any resulting path would
   exceed `path_size`, the writer must reject the entire rename operation;
   no paths are modified.
5. Clear the compacted flag, recompute the metadata CRC-32C, and rewrite
   the trailer.

Rename does not modify the entry row or its timestamps.

Directory renames modify multiple directory rows. Under in-place mutation,
a crash mid-operation may leave a partially renamed subtree. Writers
performing directory renames should prefer atomic replace mode.

### Content Replacement

To replace the content of an existing entry:

1. Write the new data block. The old data block becomes dead space (see
   Dead Data Blocks). It is not reclaimed until compaction.
2. Update the entry row with the new `data_offset`, `file_size`,
   `block_size`, `crc32c`, `mode`, and `modified_time`.
3. Recompute the metadata CRC-32C and rewrite the trailer.

## Reading an Archive

1. Read the last 64 bytes of the file to obtain the trailer.
2. Validate the magic bytes (`BALE`) at trailer bytes 56–59. Reject if
   the magic does not match.
3. Compare `archive_size` against actual file size; reject on mismatch.
4. Read alignment power and path size from the trailer. Reject if alignment
   power is greater than 16. Reject if path size is outside the range
   1–4096.
5. Read the file header at offset 0. Validate the magic bytes. Check the
   major version; reject if unsupported.
6. If `entry_table_offset` is non-zero, use it and `entry_count` to locate
   and mmap the entry table.
7. If `directory_table_offset` is non-zero, use it and
   `directory_entry_count` to locate and mmap the directory table.
8. Optionally validate the metadata CRC-32C over the file header, entry
   table, directory table, and trailer bytes 0–59 (with the CRC field
   treated as four zero bytes during computation). Validation is strongly
   recommended on first open or after any modification.
9. To read a file by path:
   a. If the compacted flag is set, binary search the directory table by
      path (unsigned byte comparison over the full `path_size` bytes) to
      find the entry ID, skipping tombstoned rows (entry ID 0). If the
      compacted flag is clear, linear scan the directory table for the
      matching path, skipping tombstoned rows.
   b. Look up the entry row at index `entry_id - 1`.
   c. If the entry is a symbolic link and the caller wants transparent
      resolution, resolve it (see Symlink Resolution). Low-level callers
      may return the symlink entry directly.
   d. If `data_offset != 0`, read `block_size` bytes at that offset.
   e. Optionally validate the data CRC-32C. Skip validation if
      `block_size = 0`.
   f. Decompress if `compression != 0`.
10. To list a directory:
    a. If the compacted flag is set, binary search for the first path with
       byte prefix `dir_name/` (trailing `/` required). Scan forward while
       the prefix matches, skipping tombstoned rows.
    b. If the compacted flag is clear, scan the entire directory table for
       paths matching the prefix, skipping tombstoned rows.
    c. Direct children are entries where the remainder contains no `/`.
    d. Symlinks are returned as-is; the caller is responsible for resolution
       if transparent traversal is desired.
11. To list root contents:
    a. Scan the directory table for live rows whose path contains no `/`.
12. To check if a directory exists:
    a. Look up the exact path (e.g., `src`) in the directory table. Verify
       the referenced entry has directory file type in its mode.

## Writing an Archive

1. Compute the full archive layout (all data sizes, offsets, table
   positions) before writing.
2. Write the file header (8 bytes) at offset 0.
3. For each file with content, write data at the next alignment boundary
   (data + zero padding). All alignment padding between the header and the
   first data block, and between data blocks, must be zero.
4. Optionally write zero-filled slack space after the last data block to
   reserve room for future data block insertions.
5. After all data blocks (and optional slack space), write the entry table
   (ordered by entry ID).
6. Optionally write zero-filled slack space after the entry table.
7. After the entry table (and optional slack space), write the directory
   table (sorted by path in lexicographic unsigned byte order over the
   full `path_size` bytes).
8. Optionally write zero-filled slack space after the directory table.
9. Compute the metadata CRC-32C over the logical concatenation of: the
   file header (8 bytes), entry table, directory table, and trailer bytes
   0–59 (with the CRC field treated as four zero bytes during computation).
10. Write the trailer (64 bytes) with the computed CRC-32C, the compacted
    flag set, and the `BALE` magic at bytes 56–59.

## Examples

### Empty Archive

An empty archive contains only the file header and trailer. The minimum
archive size is 72 bytes (8 + 64). The compacted flag is set since the
empty directory table trivially satisfies the sorted, no-tombstones
invariant. Both table offsets are 0.

```text
Offset  Size  Content
──────────────────────────────────────────
0       8     File Header (magic="BALE", sep=0x00, version=1.0.0)
8       64    Trailer (entry_table_offset=0, entry_count=0,
              dir_table_offset=0, dir_count=0,
              archive_size=72, flags=0x01,
              magic="BALE", metadata_crc=...)
──────────────────────────────────────────
Total: 72 bytes
```

### Single File Archive

An archive containing one 13-byte file (`hello.txt`) with default settings
(alignment=4096, path_size=256):

```text
Offset  Size  Content
──────────────────────────────────────────
                File Header
──────────────────────────────────────────
0       8     File Header (magic="BALE", sep=0x00, version=1.0.0)
──────────────────────────────────────────
                Alignment Padding
──────────────────────────────────────────
8       4088  Zeros (padding to first alignment boundary)
──────────────────────────────────────────
                Data Block [entry 1]
──────────────────────────────────────────
4096    13    Data: "Hello, World!"
4109    4083  Padding to next alignment boundary (zeros)
──────────────────────────────────────────
                Entry Table (1 entry)
──────────────────────────────────────────
8192    64    Entry Row (id=1, crc32c=..., data_offset=4096,
              file_size=13, block_size=13, mode=0o100644,
              compression=0, flags=0)
──────────────────────────────────────────
                Directory Table (1 row)
──────────────────────────────────────────
8256    260   Directory Row ("hello.txt" + null padding, id=1)
──────────────────────────────────────────
                Trailer
──────────────────────────────────────────
8516    64    Trailer (entry_table=8192, entry_count=1,
              dir_table=8256, dir_count=1, next_id=2,
              archive_size=8580, flags=0x01,
              magic="BALE", metadata_crc=...)
──────────────────────────────────────────
Total: 8580 bytes
```

The first data block starts at offset 4096 (the first alignment boundary
after the 8-byte file header). The 4088 bytes between the file header and
the first data block are alignment padding and must be zero.

### Archive with Hard Links

An archive where two paths (`original.txt` and `link.txt`) reference the
same entry:

- Entry table: 1 entry (id=1)
- Directory table: 2 rows (both with entry_id=1)
- Data blocks: 1 block (shared by both paths)

Hard links are only valid for regular files and symbolic links. Directories
must not be hard-linked.

### Archive with Symlink

A symlink entry (`shortcut` → `target/path`):

- Entry row: id=2, mode=0o120777, data_offset points to data block
- Data block: contains `target/path` (11 bytes) as raw UTF-8
- Directory row: path=`shortcut`, entry_id=2

### Archive with Directories

Directories appear in both tables:

- Entry row: id=3, mode=0o040755, data_offset=0, sizes=0
- Directory row: path=`src`, entry_id=3

Child entries use full paths (e.g., `src/main.rs`) and are independent
rows in both tables.

### Tombstoned Entry

After deleting entry 2 from a 3-entry archive:

- Entry table: 3 rows
  - Row 0: id=1, live entry
  - Row 1: id=0, all bytes zeroed (tombstone)
  - Row 2: id=3, live entry
- Directory table: corresponding row(s) for the deleted entry have path
  zeroed and entry ID set to 0
- `entry_count` remains 3 (physical row count)
- Compacted flag is cleared
- Data block for the deleted entry remains as dead space until compaction

---

Last updated: 2026-02-10T17:17:05.977Z
Copyright 2026 Adam Mill
