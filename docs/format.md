# Bale Archive Format Specification

Version: 1.0.0

## Overview

Bale is a mmap-first, zero-copy archive format designed for efficient random
access. It uses fixed-stride tables for O(1) entry lookup and alignment-padded
data blocks for direct memory mapping.

A bale archive consists of five sections, laid out sequentially:

1. **File Header** - Format identification and version (8 bytes)
2. **Data Blocks** - File contents, each aligned to a boundary
3. **Entry Table** - Fixed-stride metadata for each entry (no paths)
4. **Directory Table** - Fixed-stride path-to-entry-ID mapping
5. **Trailer** - Configuration, table offsets, and counts (64 bytes)

All multi-byte integer fields are **little-endian**.

## Archive Layout

```text
┌──────────────────────────────────────┐  Offset 0
│ File Header (8 bytes)                │
├──────────────────────────────────────┤  Aligned
│ Data Block [0]                       │
│   Header (32 bytes)                  │
│   Data (block_size bytes)            │
│   Padding to alignment              │
├──────────────────────────────────────┤  Aligned
│ Data Block [1]                       │
│   ...                                │
├──────────────────────────────────────┤
│ ...                                  │
├──────────────────────────────────────┤
│ Entry Table                          │
│   Entry Row [0] (48 bytes)           │
│   Entry Row [1] (48 bytes)           │
│   ...                                │
├──────────────────────────────────────┤
│ Directory Table                      │
│   Directory Row [0] (path_size + 4)  │
│   Directory Row [1] (path_size + 4)  │
│   ...                                │
├──────────────────────────────────────┤
│ Trailer (64 bytes)                   │  Last 64 bytes of file
└──────────────────────────────────────┘
```

## Configuration

Two parameters control the archive geometry:

| Setting   | Default      | Range              | Description                     |
|-----------|--------------|--------------------|---------------------------------|
| Alignment | 4096 (2^12)  | 2^0 to 2^24       | Data block alignment in bytes   |
| Path size | 256          | 1 to 4096          | Maximum path length in bytes    |

**Alignment** determines data block padding. Stored as a power-of-two exponent
(e.g., 12 means 2^12 = 4096 bytes).

**Path size** is the fixed width of the path field in the directory table. Paths
shorter than `path_size` are null-padded. Paths longer than `path_size` cannot
be stored.

## File Header

8 bytes at offset 0. Identifies the file as a bale archive and declares
the format version.

| Offset | Size | Field         | Description                            |
|--------|------|---------------|----------------------------------------|
| 0      | 5    | Magic         | `"BALE\0"` (0x42 0x41 0x4C 0x45 0x00) |
| 5      | 1    | Major version | Format major version                   |
| 6      | 1    | Minor version | Format minor version                   |
| 7      | 1    | Patch version | Format patch version                   |

The magic bytes allow `file(1)` and similar tools to identify bale archives
by reading the first 5 bytes. The version bytes enable readers to select
the appropriate parser for the format version.

## Data Blocks

One data block per entry that has content (regular files and symlinks).
Directories and empty files have no data block.

Each data block starts at an alignment boundary. The first data block begins
at the first alignment boundary at or after byte 8 (the end of the file
header).

### Data Block Header

32 bytes at the start of each data block.

| Offset | Size | Field              | Description                       |
|--------|------|--------------------|-----------------------------------|
| 0      | 4    | Entry ID           | LE, matches entry table row       |
| 4      | 8    | File size          | LE, original uncompressed size    |
| 12     | 8    | Block size         | LE, stored size (after compression) |
| 20     | 4    | CRC-32             | Checksum of the data bytes        |
| 24     | 1    | Compression method | 0 = none (stored)                 |
| 25     | 7    | Reserved           | Must be zero                      |

### Data Block Layout

```text
┌────────────────────────────────────────┐  Aligned offset
│ Data Block Header (32 bytes)           │
├────────────────────────────────────────┤
│ Data (block_size bytes)                │
├────────────────────────────────────────┤
│ Padding (zeros to next alignment)      │
└────────────────────────────────────────┘
```

**Total block size:** `align_up(32 + block_size, alignment)`

When compression is not used, `file_size` and `block_size` are equal.

The CRC-32 is computed over the stored data bytes (the `block_size` bytes
following the header), not the original uncompressed data. When compression
is not used, these are the same.

### Symlink Data

Symlinks store their target path as the data block content. The target path
is stored as raw UTF-8 bytes without a null terminator. The `file_size` and
`block_size` equal the byte length of the target path.

## Entry Table

A contiguous array of fixed-stride rows, one per entry. Contains metadata
but no paths. The entry table offset and count are stored in the trailer.

### Entry Row

48 bytes per entry.

| Offset | Size | Field             | Description                         |
|--------|------|-------------------|-------------------------------------|
| 0      | 4    | Entry ID          | LE, unique within the archive       |
| 4      | 8    | Data block offset | LE, byte offset to data block (0 = none) |
| 12     | 8    | File size         | LE, original uncompressed size      |
| 20     | 8    | Block size        | LE, stored size (after compression) |
| 28     | 8    | Created time      | LE, i64, Unix epoch milliseconds    |
| 36     | 8    | Modified time     | LE, i64, Unix epoch milliseconds    |
| 44     | 4    | Mode              | LE, Unix permissions and file type  |

**Stride:** 48 bytes (fixed for all entries).

**Ordering:** Rows are sorted by entry ID in ascending order, enabling binary
search.

### Entry IDs

- Entry IDs are unsigned 32-bit integers.
- ID 0 is reserved as a sentinel (root / no-ID) and must not appear in the
  entry table.
- IDs are assigned sequentially starting from 1.
- IDs are never reused within a session. The next available ID is tracked
  in the trailer's `next_id` field.
- A compact operation may renumber all entries 1..N and reset `next_id` to
  N+1.

### File Types and Mode

The `mode` field uses standard Unix mode encoding:

| Bits    | Mask     | Description      |
|---------|----------|------------------|
| 15-12   | 0xF000   | File type        |
| 11-9    | 0x0E00   | Special bits     |
| 8-0     | 0x01FF   | Permission bits  |

File type values (upper 4 bits):

| Value  | Type             |
|--------|------------------|
| 0x8    | Regular file     |
| 0x4    | Directory        |
| 0xA    | Symbolic link    |

### Directories

Directories have `data_offset = 0`, `file_size = 0`, and `block_size = 0`.
The directory's existence is recorded in both the entry table (with mode
indicating directory type) and the directory table (path mapped to entry ID).

### Empty Files

Regular files with no content have `data_offset = 0`, `file_size = 0`, and
`block_size = 0`. They are distinguished from directories by their mode.

## Directory Table

A contiguous array of fixed-stride rows mapping paths to entry IDs. The
directory table offset and count are stored in the trailer.

### Directory Row

`path_size + 4` bytes per row.

| Offset      | Size        | Field    | Description                    |
|-------------|-------------|----------|--------------------------------|
| 0           | `path_size` | Path     | UTF-8, null-padded             |
| `path_size` | 4           | Entry ID | LE, references an entry row    |

**Stride:** `path_size + 4` bytes (fixed for all rows).

**Ordering:** Rows are sorted by path in lexicographic byte order, enabling
binary search for lookups and efficient directory listing.

### Paths

- Paths are UTF-8 encoded.
- Paths use `/` as the directory separator.
- Paths are relative (no leading `/`).
- Paths must not contain `.` or `..` components.
- Paths must not have trailing `/` separators.
- Paths shorter than `path_size` are padded with null bytes (`0x00`).
- Directory entries include paths for the directory itself (e.g., `src/`
  is stored as `src`).

### Hard Links

Multiple directory rows may reference the same entry ID with different paths.
This models hard links: several paths sharing the same file content and
metadata.

### Reserved Paths

Path components starting with `.bale` are reserved for internal use. Archives
must not contain entries with reserved path components. This applies to any
path component (segment between `/` separators), not just the first.

Reserved: `.bale`, `.bale.txt`, `.bale/file`, `foo/.bale/bar`, `.baledata`

Not reserved: `.bal`, `bale`, `foo.bale`, `mybale/file.txt`

## Trailer

64 bytes, always the last 64 bytes of the archive. Contains all archive
configuration, table offsets, and counts needed to read the archive.

| Offset | Size | Field                | Description                        |
|--------|------|----------------------|------------------------------------|
| 0      | 5    | Magic                | `"BALE\0"` (0x42 0x41 0x4C 0x45 0x00) |
| 5      | 1    | Major version        | Format major version               |
| 6      | 1    | Minor version        | Format minor version               |
| 7      | 1    | Patch version        | Format patch version               |
| 8      | 8    | Entry table offset   | LE, byte offset to entry table     |
| 16     | 4    | Entry count          | LE, number of entry rows           |
| 20     | 8    | Directory table offset | LE, byte offset to directory table |
| 28     | 4    | Directory entry count | LE, number of directory rows       |
| 32     | 4    | Next entry ID        | LE, next ID to assign              |
| 36     | 1    | Alignment power      | Exponent N where alignment = 2^N   |
| 37     | 2    | Path size            | LE, maximum path length (1-4096)   |
| 39     | 25   | Reserved             | Must be zero                       |

The trailer version should match the file header version. Readers should
validate this consistency.

## Reading an Archive

1. Read the last 64 bytes of the file to obtain the trailer.
2. Validate the magic bytes and version.
3. Read alignment and path size from the trailer.
4. Use `entry_table_offset` and `entry_count` to locate and mmap the entry
   table.
5. Use `directory_table_offset` and `directory_entry_count` to locate and
   mmap the directory table.
6. To read a file:
   a. Binary search the directory table by path to find the entry ID.
   b. Binary search the entry table by ID to find the entry row.
   c. If `data_offset != 0`, read the data block header and data at that
      offset.
7. Optionally read the file header at offset 0 to validate that the version
   matches the trailer.

## Writing an Archive

1. Write the file header (8 bytes) at offset 0.
2. For each file with content, write a data block at the next alignment
   boundary (header + data + padding).
3. After all data blocks, write the entry table (sorted by entry ID).
4. After the entry table, write the directory table (sorted by path).
5. Write the trailer (64 bytes).

## Examples

### Empty Archive

An empty archive contains only the file header and trailer. The minimum
archive size is 72 bytes (8 + 64).

```text
Offset  Size  Content
──────────────────────────────────────────
0       8     File Header (magic="BALE\0", version=1.0.0)
8       64    Trailer (entry_count=0, dir_count=0)
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
0       8     File Header (magic="BALE\0", version=1.0.0)
──────────────────────────────────────────
                Data Block [entry 1]
──────────────────────────────────────────
4096    32    Data Block Header (id=1, file_size=13, block_size=13)
4128    13    Data: "Hello, World!"
4141    3955  Padding to next alignment boundary
──────────────────────────────────────────
                Entry Table (1 entry)
──────────────────────────────────────────
8192    48    Entry Row (id=1, data_offset=4096, mode=0o100644)
──────────────────────────────────────────
                Directory Table (1 row)
──────────────────────────────────────────
8240    260   Directory Row ("hello.txt" + padding, id=1)
──────────────────────────────────────────
                Trailer
──────────────────────────────────────────
8500    64    Trailer (entry_table=8192, entry_count=1,
              dir_table=8240, dir_count=1, next_id=2)
──────────────────────────────────────────
Total: 8564 bytes
```

The first data block starts at offset 4096 (the first alignment boundary
after the 8-byte file header).

### Archive with Hard Links

An archive where two paths (`original.txt` and `link.txt`) reference the
same entry:

- Entry table: 1 entry (id=1)
- Directory table: 2 rows (both with entry_id=1)
- Data blocks: 1 block (shared by both paths)

### Archive with Symlink

A symlink entry (`shortcut` -> `target/path`):

- Entry row: id=2, mode=0o120777, data_offset points to data block
- Data block: contains `target/path` (11 bytes) as raw UTF-8
- Directory row: path=`shortcut`, entry_id=2

### Archive with Directories

Directories appear in both tables:

- Entry row: id=3, mode=0o040755, data_offset=0, sizes=0
- Directory row: path=`src`, entry_id=3

Child entries use full paths (e.g., `src/main.rs`) and are independent
rows in both tables.
