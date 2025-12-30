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
| Alignment  | 4096 bytes                        |
| Max path   | 256 bytes                         |
