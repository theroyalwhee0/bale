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

### Library

```rust
use bale::touch;
use std::path::Path;

// Create an empty archive
touch(Path::new("archive.bale"))?;
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
