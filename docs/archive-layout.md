# Bale Archive Layout

Bale is a ZIP-compatible archive format with fixed-stride entries for efficient
random access. This document describes the binary layout of a bale archive.

## Overview

A bale archive consists of three main sections:

1. **File Data** - Local file entries (header + path + data), repeated per file
2. **Central Directory** - Fixed-stride entry index, repeated per file
3. **Trailer** - Archive metadata (256 bytes, fixed size)

## Complete Archive Structure

| Section | Description | Size |
|---------|-------------|------|
| **File Data Section** | | |
| ┗ Local File Entry | Header + path + data + padding | Aligned to `alignment` |
| ┗ ... | *Repeated for each file* | × `entry_count` |
| **Central Directory** | | |
| ┗ CD Entry | Header (46) + path (`path_size`) | `46 + path_size` |
| ┗ ... | *Repeated for each file* | × `entry_count` |
| **Trailer (256 bytes)** | | |
| ┗ ZIP64 EOCD | ZIP64 End of Central Directory | 56 bytes |
| ┗ ZIP64 EOCD Locator | Pointer to ZIP64 EOCD | 20 bytes |
| ┗ EOCD | Standard End of Central Directory | 22 bytes |
| ┗ BaleEocd | Bale-specific metadata | 158 bytes |

## Central Directory Alignment

The Central Directory starts at an aligned offset for efficient mmap access.
Since each local file entry is padded to the alignment boundary, the CD naturally
begins at an aligned position immediately after the file data section. Note that
the CD itself is not padded, so the 256-byte trailer is not alignment-guaranteed.

The ZIP64 EOCD Locator must be exactly 20 bytes and positioned immediately before
the EOCD (at EOCD offset - 20). This is a ZIP format requirement that standard
tools rely on when searching for ZIP64 structures.

## Section Details

### Local File Entry (× N, one per file)

Each file in the archive has one Local File Entry containing the header,
path, data, and padding. These entries are repeated sequentially.

**Entry layout:**

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | Signature `0x04034b50` |
| 4 | 2 | Version needed to extract |
| 6 | 2 | General purpose bit flag |
| 8 | 2 | Compression method (0 = stored) |
| 10 | 2 | Last modified time (DOS format) |
| 12 | 2 | Last modified date (DOS format) |
| 14 | 4 | CRC-32 checksum |
| 18 | 4 | Compressed size |
| 22 | 4 | Uncompressed size |
| 26 | 2 | Path length (fixed: `path_size`) |
| 28 | 2 | Extra field length (0) |
| 30 | `path_size` | Path (null-padded) |
| 30 + `path_size` | varies | File data |
| ... | varies | Padding to alignment boundary |

**Header size:** 30 bytes
**Entry size:** Aligned to `alignment` bytes (default: 4096)

For files larger than `alignment - 30 - path_size` bytes (3810 bytes with defaults),
the entry spans multiple alignment blocks. The total entry size is always rounded
up to the next alignment boundary.

### Central Directory Entry (× N, one per file)

Each file has a corresponding CD Entry in the Central Directory. These
entries are fixed-stride, enabling O(1) random access by index.

**Entry layout:**

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | Signature `0x02014b50` |
| 4 | 2 | Version made by |
| 6 | 2 | Version needed to extract |
| 8 | 2 | General purpose bit flag |
| 10 | 2 | Compression method |
| 12 | 2 | Last modified time |
| 14 | 2 | Last modified date |
| 16 | 4 | CRC-32 checksum |
| 20 | 4 | Compressed size |
| 24 | 4 | Uncompressed size |
| 28 | 2 | Path length (fixed: `path_size`) |
| 30 | 2 | Extra field length (0) |
| 32 | 2 | File comment length (0) |
| 34 | 2 | Disk number start (0) |
| 36 | 2 | Internal file attributes |
| 38 | 4 | External file attributes (Unix mode) |
| 42 | 4 | Local header offset |
| 46 | `path_size` | Path (null-padded) |

**Entry stride:** 46 + `path_size` bytes (fixed for all entries)
**CD offset calculation:** `cd_offset + (index × stride)`

### ZIP64 End of Central Directory (56 bytes)

Standard ZIP64 EOCD size for ZIP compatibility.

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | Signature `0x06064b50` |
| 4 | 8 | Size of remaining record (44) |
| 12 | 2 | Version made by |
| 14 | 2 | Version needed to extract |
| 16 | 4 | Disk number (0) |
| 20 | 4 | CD start disk (0) |
| 24 | 8 | CD entries on this disk |
| 32 | 8 | Total CD entries |
| 40 | 8 | CD size in bytes |
| 48 | 8 | CD offset from archive start |

### ZIP64 End of Central Directory Locator (20 bytes)

Standard ZIP64 EOCD Locator size for ZIP compatibility.

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | Signature `0x07064b50` |
| 4 | 4 | Disk with ZIP64 EOCD (0) |
| 8 | 8 | ZIP64 EOCD offset |
| 16 | 4 | Total disks (1) |

### End of Central Directory (22 bytes)

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | Signature `0x06054b50` |
| 4 | 2 | Disk number (0) |
| 6 | 2 | CD start disk (0) |
| 8 | 2 | CD entries on this disk |
| 10 | 2 | Total CD entries |
| 12 | 4 | CD size in bytes |
| 16 | 4 | CD offset |
| 20 | 2 | Comment length (158) |

When values exceed the field capacity (e.g., >65535 entries), overflow markers
are used (`0xFFFF` for 16-bit, `0xFFFFFFFF` for 32-bit) and the ZIP64 EOCD
contains the actual values.

### BaleEocd (158 bytes)

Stored as the EOCD comment field.

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | Magic `"BALE"` (`0x454C4142`) |
| 4 | 1 | Major version |
| 5 | 1 | Minor version |
| 6 | 1 | Patch version |
| 7 | 1 | Alignment power (2^N bytes) |
| 8 | 2 | Path size (1-2048) |
| 10 | 148 | Reserved (zeros) |

## Default Configuration

| Setting | Default | Range |
|---------|---------|-------|
| Alignment | 4096 bytes (2^12) | 1 to 16 MB (2^0 to 2^24) |
| Path size | 256 bytes | 1 to 2048 bytes |

**Path length constraint:** If a file's path exceeds the configured `path_size`,
archive creation fails with an error. Choose a `path_size` large enough for your
longest expected path.

## Example: Empty Archive

An empty bale archive with default settings is exactly 256 bytes (trailer only).
The ZIP64 EOCD starts at offset 0, and the locator points to offset 0. Standard
ZIP tools handle this correctly since they search backward from the EOCD.

```text
Offset    Size    Content
─────────────────────────────────────────────────────
                  Trailer (256 bytes)
─────────────────────────────────────────────────────
0         56      ZIP64 EOCD (entry_count=0, cd_size=0, cd_offset=0)
56        20      ZIP64 EOCD Locator (zip64_eocd_offset=0)
76        22      EOCD (entries=0, cd_size=0, cd_offset=0, comment_len=158)
98       158      BaleEocd (magic="BALE", align=4096, path_size=256)
─────────────────────────────────────────────────────
Total: 256 bytes
```

## Example: Single File Archive

A bale archive containing one 13-byte file ("hello.txt") with default settings:

```text
Offset    Size    Content
─────────────────────────────────────────────────────
                  Local File Entry [0] (4096 bytes, aligned)
─────────────────────────────────────────────────────
0         30      Header
30       256      Path: "hello.txt" + null padding
286       13      File data: "Hello, World!"
299     3797      Padding to alignment boundary
─────────────────────────────────────────────────────
                  Central Directory (1 entry × 302 bytes)
─────────────────────────────────────────────────────
4096     302      CD Entry [0]: header (46) + path (256)
─────────────────────────────────────────────────────
                  Trailer (256 bytes)
─────────────────────────────────────────────────────
4398      56      ZIP64 EOCD
4454      20      ZIP64 EOCD Locator
4474      22      EOCD
4496     158      BaleEocd
─────────────────────────────────────────────────────
Total: 4654 bytes
```

Note: The Central Directory starts at offset 4096 (aligned to the 4096-byte boundary).

## Example: Multi-File Archive

An archive with 3 small files (each fitting in one 4096-byte block) would have
this structure:

```text
Offset    Size    Content
─────────────────────────────────────────────────────
                  Local File Entries (× 3)
─────────────────────────────────────────────────────
0        4096     Local File Entry [0]
4096     4096     Local File Entry [1]
8192     4096     Local File Entry [2]
─────────────────────────────────────────────────────
                  Central Directory (3 entries × 302 bytes)
─────────────────────────────────────────────────────
12288     302     CD Entry [0]
12590     302     CD Entry [1]
12892     302     CD Entry [2]
─────────────────────────────────────────────────────
                  Trailer (256 bytes)
─────────────────────────────────────────────────────
13194      56     ZIP64 EOCD
13250      20     ZIP64 EOCD Locator
13270      22     EOCD
13292     158     BaleEocd
─────────────────────────────────────────────────────
Total: 13450 bytes
```

CD Entry offset for file at index `i`: `12288 + (i × 302)`

## ZIP Compatibility

Bale archives are valid ZIP files and can be read by standard ZIP tools:

- `unzip` - Extract files
- `zipinfo` - List contents
- `file` - Identify as ZIP archive

The ZIP64 structures ensure compatibility with archives containing more than
65535 entries or exceeding 4 GB in size.
