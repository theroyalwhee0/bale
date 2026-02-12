# bale

> **Warning**: This project is a work-in-progress. It has known bugs, incomplete
> features, and the API will change. Use at your own risk.

A Rust library and CLI for working with bale archives - a mmap-first, zero-copy
archive format with fixed-stride tables for efficient random access.

## Features

- **Memory-mapped**: Designed for efficient mmap-based access
- **Zero-copy**: Fixed-stride tables enable direct access without parsing
- **4K aligned**: Data blocks aligned to 4096 bytes for optimal I/O
- **Hard links**: Multiple paths can reference the same entry
- **Symlinks**: Symbolic links with target paths stored as data
- **Millisecond timestamps**: i64 created and modified times

## Installation

Requires Rust 1.89.0 or later.

```bash
cargo install bale
```

Or add to your `Cargo.toml`:

```toml
[dependencies]
bale = "0.1"
```

## Usage

### CLI

Create an empty bale archive:

```bash
bale touch archive.bale
```

Add files to an archive:

```bash
bale add archive.bale file1.txt file2.txt

# Add with a path prefix
bale add archive.bale --prefix src/ *.rs
```

List entries in an archive:

```bash
bale list archive.bale
```

Extract entries from an archive:

```bash
# Extract all entries to current directory
bale extract archive.bale

# Extract to a specific directory
bale extract archive.bale -o output/

# Extract specific entries
bale extract archive.bale file1.txt file2.txt
```

Delete entries from an archive:

```bash
bale delete archive.bale file1.txt file2.txt
```

Compact an archive to reclaim space from deleted entries:

```bash
bale compact archive.bale
```

Mount an archive as a FUSE filesystem:

```bash
# Mount at a specific directory
bale mount archive.bale /mnt/archive

# Interactive shell at mount point
bale mount archive.bale --shell

# Run a command against mounted archive
bale mount archive.bale --shell 'ls -la'

# Use python3 via shell
bale mount archive.bale --shell "python3 -c 'import os; print(os.listdir(\".\"))'"
```

### Library

```rust
use bale::{ArchiveWriter, ArchiveReader};

// Create a new archive and add an entry
let mut writer = ArchiveWriter::create("archive.bale")?;
writer.add_entry("hello.txt", b"Hello, World!", 0o644)?;
writer.sync()?;

// Read from an archive
let reader = ArchiveReader::open("archive.bale")?;
if let Some(entry) = reader.find_entry("hello.txt") {
    let data = reader.read_data(entry)?;
    println!("{}", String::from_utf8_lossy(data));
}
```

## Format

Bale is a native archive format designed for mmap-based random access.
See [docs/bale-spec.md](docs/bale-spec.md) for the full specification.

| Property        | Value                            |
| --------------- | -------------------------------- |
| File header     | 8 bytes (`BALE\0` + version)     |
| Trailer         | 64 bytes                         |
| Alignment       | 4096 bytes (configurable, 2^N)   |
| Max path length | 256 bytes (configurable, 1-4096) |
| Byte order      | Little-endian                    |
| Timestamps      | i64 Unix epoch milliseconds      |

## Development

```bash
# Build
cargo build

# Test
cargo test

# Lint
cargo clippy

# Format
cargo fmt
```

## Status

Version 0.1.0. The format is stable but the API may change in future releases.

## License & Copyright

Licensed under the Apache License, Version 2.0. See [LICENSE.txt](LICENSE.txt)
for details.

Copyright 2025 Adam Mill
