# BALE - Project Context for Claude

This file is authored and maintained by Claude (Anthropic's AI assistant) to
provide context for future Claude sessions working on this project.

## Project Overview

`bale` is a Rust library implementing a mmap-first, zero-copy zip-compatible
archive format. It uses fixed-stride entries for efficient random access.

## Code Organization

### One Item Per File

- Place one public item (struct, enum, or trait) per file
- File names should match the item name (e.g., `Entry` in `entry.rs`)
- Each file contains the item and all its implementations
- Type aliases are exempt and can be grouped logically
- When violating this guideline, include a comment explaining why

### Documentation

Document all functions with `///` doc comments, including private and test
functions. Every function should explain its purpose.

### Scoped Mutability

Prefer hiding `mut` bindings and `#[cfg]` feature branching inside block
expressions to limit their scope:

```rust
// Good: mut is scoped to initialization
let path_buf = {
    let mut buf = [0u8; PATH_SIZE];
    buf[..len].copy_from_slice(bytes);
    buf
};

// Good: cfg branching returns a value
let mode = {
    #[cfg(unix)]
    { metadata.permissions().mode() }
    #[cfg(not(unix))]
    { 0o644 }
};
```

### mod.rs as Table of Contents

`mod.rs` files should **only** contain module declarations and re-exports.

- All implementation code (structs, enums, functions, impls, tests) must be
  in separate files
- `mod.rs` serves as the module's table of contents

## Dependencies

- Minimal, well-vetted dependencies
- Feature-gated where appropriate
- **IMPORTANT**: Never add dependencies without giving user a chance to review
  them BEFORE adding
- All dependencies belong in the workspace root `Cargo.toml`
- Always pin exact versions (e.g., `"3.1.3"` not `"3"`)

## Git Configuration

**No Force Push or Amend**: Repository rules prevent force-pushing to ANY
branch.

- **NEVER use `git push --force` or `git push --force-with-lease`**
- **NEVER use `git commit --amend` on commits that have been pushed**
- If you need to fix a pushed commit, create a NEW commit instead

**Whitelist .gitignore**: This project uses a whitelist approach. New file
types must be explicitly added to `.gitignore` with specific extensions (not
wildcards like `*`).

## Development Workflow

```bash
cargo build              # Build
cargo nextest run        # Test (preferred)
cargo test               # Test (alternative)
cargo clippy             # Lint
cargo fmt                # Format
git precommit --all      # Run all checks
```

## Testing

### Unit Tests

- Use `#[cfg(test)]` modules in source files
- Document test panic expectations
- Test round-trip serialization/deserialization

### Integration Tests

Located in `tests/`:

- `tests/fixtures.rs` - Generates test fixtures (run with `--ignored`)
- `tests/fixtures/` - Binary fixtures (e.g., `empty.bale`)
- `tests/empty_archive.rs` - trycmd-based CLI tool tests
- `tests/cmd/` - trycmd test cases as `.toml` files

Regenerate fixtures: `cargo test --test fixtures -- --ignored`

### trycmd Pattern

For testing with external CLI tools, use `/usr/bin/env` to resolve from PATH:

```toml
bin.path = "/usr/bin/env"
args = ["toolname", "arguments..."]
status.code = 0
```

Expected output goes in matching `.stdout` files.

## Format Specification

| Property   | Value                             |
| ---------- | --------------------------------- |
| EOCD       | Standard 22-byte zip format       |
| Stride     | `header_size + path_size` (fixed) |
| Byte order | Little-endian                     |
| Alignment  | 4096 bytes (configurable, 2^N)    |
| Max path   | 256 bytes (configurable, 1-2048)  |

### Archive Layout

```text
┌─────────────────────────────────────┐
│ Local File Header + Data (aligned)  │ ← Repeats for each file
├─────────────────────────────────────┤
│ Central Directory Headers           │ ← Fixed stride entries
├─────────────────────────────────────┤
│ [ZIP64 EOCD Record - if needed]     │ ← Optional, for large archives
│ [ZIP64 EOCD Locator - if needed]    │
├─────────────────────────────────────┤
│ EOCD (22 bytes) + BaleEocd (234 b)  │ ← 256-byte trailer
└─────────────────────────────────────┘
```

### BaleEocd (EOCD Comment)

234-byte structure stored as the EOCD comment field. Combined with the 22-byte
EOCD, the total trailer is exactly 256 bytes for efficient single-read access.

| Offset | Size | Field                                   |
| ------ | ---- | --------------------------------------- |
| 0      | 4    | Magic signature "BALE" (0x454C4142 LE)  |
| 4      | 1    | Major version                           |
| 5      | 1    | Minor version                           |
| 6      | 1    | Patch version                           |
| 7      | 1    | Alignment power (2^N, e.g., 12 = 4096)  |
| 8      | 2    | Path size (1-2048, little-endian)       |
| 10     | 224  | Reserved (zeros)                        |

### ZIP64 Compatibility

ZIP64 structures are placed *before* the standard EOCD, so the comment field
remains available at the end of the archive. The BaleMetadata comment is valid
for both standard and ZIP64 archives.

Reference: [APPNOTE.TXT](https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT)
