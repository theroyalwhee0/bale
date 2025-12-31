# bale

A Rust library and CLI for working with bale archives - a mmap-first, zero-copy
zip-compatible archive format with fixed-stride entries for efficient random
access.

## Features

- **Zip-compatible**: Archives can be read by standard zip tools (unzip, 7z, etc.)
- **Memory-mapped**: Designed for efficient mmap-based access
- **Zero-copy**: Fixed-stride entries enable direct access without parsing
- **4K aligned**: File data aligned to 4096 bytes for optimal I/O

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

Bale extends the zip format with constraints that enable efficient random access:

| Property        | Value                       |
| --------------- | --------------------------- |
| Alignment       | 4096 bytes                  |
| Max path length | 256 bytes                   |
| Byte order      | Little-endian               |
| EOCD            | Standard 22-byte zip format |

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

Early development. The API is not yet stable.
