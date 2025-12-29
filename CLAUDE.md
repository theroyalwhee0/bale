# BALE - Project Context for Claude

## Project Overview

`bale` is a Rust library implementing a mmap-first, zero-copy zip-compatible
archive format. It uses fixed-stride entries for efficient random access.

## File Organization (One Item Per File)

- **Guideline**: Place one public item (struct, enum, or trait) per file.
  - File names should match the item name (e.g., `Entry` in `entry.rs`)
  - Each file contains the item and all its implementations
  - Type aliases are exempt from this rule and can be grouped logically
  - When violating this guideline, include a comment explaining why

## Module Organization (mod.rs as Table of Contents)

`mod.rs` files should **only** contain module declarations and re-exports.

- All implementation code (structs, enums, functions, impls, tests) must be
  in separate files
- `mod.rs` serves as the module's table of contents

### Dependencies Philosophy

- Minimal, well-vetted dependencies
- Feature-gated where appropriate
- **IMPORTANT**: Never add dependencies without giving a chance to review
  them BEFORE adding
- All dependencies belong in the workspace root `Cargo.toml`, not in
  individual crate `Cargo.toml` files
- Always pin exact versions (e.g., `"3.1.3"` not `"3"`)

### Git Configuration

**CRITICAL - No Force Push or Amend**: Repository rules prevent force-pushing
to ANY branch. This means:

- **NEVER use `git push --force` or `git push --force-with-lease`**
- **NEVER use `git commit --amend` on commits that have been pushed**
- If you need to fix a pushed commit, create a NEW commit instead
- Multiple small commits are fine - they get squashed on merge

If you accidentally amend a pushed commit, you'll need to reset and recommit.

**Whitelist .gitignore**: This project uses a whitelist approach to version
control.

## Development Workflow

- Build: `cargo build`
- Test: `cargo nextest run` (preferred) or `cargo test`
- Lint: `cargo clippy`
- Format: `cargo fmt`
- Precommit: `git precommit --all`

### Testing Notes

- Use `#[cfg(test)]` modules
- Document test panic expectations
- Test round-trip serialization/deserialization

## Format Notes

- **Zip-compatible EOCD**: Standard 22-byte format for empty archives
- **Fixed stride**: `header_size + path_size` for CD and local headers
- **Little-endian**: All integer fields are little-endian
- **Constants** (current): `ALIGNMENT = 4096`, `PATH_SIZE = 256`
