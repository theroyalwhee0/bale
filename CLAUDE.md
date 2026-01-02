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

### Strict Linting and Quality Standards

- **Forbidden unsafe code**: `unsafe_code = "forbid"`
- **Required documentation**: All items (public and private) must be documented
  - Missing docs are denied (both code and rustdoc)
  - All functions must include `# Errors`, `# Panics`, and `# Safety` docs
    where applicable
- **No direct stdout/stderr**: `print_stdout` and `print_stderr` are denied
- **Warnings are errors**: Pre-commit hooks run clippy with `-D warnings`,
  so any warning fails the commit

## Important Conventions

1. **Documentation First**: Write docs before implementation
2. **Error Propagation**: Use `Result` types with descriptive errors
3. **Zero-Copy Design**: Prefer borrowing over allocation where possible
4. **Little-Endian**: All integer fields in the format are little-endian

## Dependencies

- Minimal, well-vetted dependencies
- Feature-gated where appropriate
- **IMPORTANT**: Never add dependencies without giving user a chance to review
  them BEFORE adding
- All dependencies belong in the workspace root `Cargo.toml`
- Always pin exact versions (e.g., `"3.1.3"` not `"3"`)

## Issue Workflow

**IMPORTANT**: Always use `focus-issue` to start work on GitHub issues.
Do NOT use `gh issue` directly.

```bash
focus-issue <issue-number>
```

This command:

1. Assigns the issue to you (if not already assigned)
2. Creates or switches to an issue branch (e.g., `2-implement-index-table`)
3. Fetches issue content to `.focus/Issue.md` for easy reference

The `.focus/` directory is gitignored and contains local working context
for the current issue.

After completing work:

0. Pause and let your coworker review the git diffs.
1. Commit changes to the issue branch
2. Push and create a PR via `gh pr create`
3. After merge, switch back to main: `git checkout main && git pull`

## Git Configuration

**No `-C` Flag**: Do not use `git -C <path>`. Run git commands from the
working directory instead.

**No Force Push or Amend**: Repository rules prevent force-pushing to ANY
branch.

- **NEVER use `git push --force` or `git push --force-with-lease`**
- **NEVER use `git commit --amend` on commits that have been pushed**
- If you need to fix a pushed commit, create a NEW commit instead
- Multiple small commits are fine - they get squashed on merge

If you accidentally amend a pushed commit, you'll need to reset and recommit.

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
cargo-coverage --overview       # Coverage summary
cargo-coverage <file>...        # Coverage for specific files
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
│ ZIP64 EOCD (56 bytes)               │ ← Always present
│ ZIP64 EOCD Locator (20 bytes)       │
│ EOCD (22 bytes)                     │
│ BaleEocd (158 bytes)                │ ← 256-byte trailer total
└─────────────────────────────────────┘
```

### BaleEocd (EOCD Comment)

158-byte structure stored as the EOCD comment field. Combined with ZIP64 EOCD
(56), ZIP64 Locator (20), and EOCD (22), the trailer is exactly 256 bytes.

| Offset | Size | Field                                   |
| ------ | ---- | --------------------------------------- |
| 0      | 4    | Magic signature "BALE" (0x454C4142 LE)  |
| 4      | 1    | Major version                           |
| 5      | 1    | Minor version                           |
| 6      | 1    | Patch version                           |
| 7      | 1    | Alignment power (2^N, e.g., 12 = 4096)  |
| 8      | 2    | Path size (1-2048, little-endian)       |
| 10     | 148  | Reserved (zeros)                        |

### ZIP64 Compatibility

ZIP64 structures are always present to maintain a fixed 256-byte trailer size.
The ZIP64 EOCD Locator must be exactly 20 bytes, positioned immediately before
the EOCD (required by ZIP tools that search backward from EOCD).

Reference: [APPNOTE.TXT](https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT)
