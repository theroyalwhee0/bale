# Tests

This directory contains integration tests for the bale CLI and library.

## Running Tests

```bash
# Run all tests (integration tests requiring external tools are skipped)
cargo test

# Run all tests including integration tests
cargo test --features integration-tests

# Regenerate test fixtures
cargo test --test fixtures -- --ignored
```

## Test Categories

### Unit Tests (always run)

- `cli_commands.rs` - Tests bale CLI commands (touch, add, ls, delete, extract, check, compact)
- `check_valid.rs` - Tests `bale check` on valid archives
- `check_invalid.rs` - Tests `bale check` on invalid archives

### Integration Tests (require `--features integration-tests`)

These tests require external tools (`zipinfo`, `unzip`, `file`, `fusermount`):

- `empty.rs` - Verifies empty archives with external zip tools
- `single_file.rs` - Verifies single-file archives with external zip tools
- `align_16k.rs` - Verifies 16KB-aligned archives with zipinfo
- `path_2048.rs` - Verifies archives with 2048-byte path size
- `fuse_read.rs` - FUSE mount read operations via `bale mount --shell`
- `fuse_write.rs` - FUSE mount write operations (create, delete, rename, chmod)

### Fixture Generation (ignored by default)

- `fixtures.rs` - Generates all test fixtures; run with `--ignored`

## Test Fixtures

### Valid Archives (`fixtures/valid/`)

| File | Description |
|------|-------------|
| `empty.bale` | Empty archive (256-byte trailer only) |
| `single_file.bale` | Single "hello.txt" with "Hello, World!" |
| `multi_file.bale` | Multiple files and directories with various permissions |
| `align_16k.bale` | Archive with 16KB data alignment |
| `path_2048.bale` | Archive with 2048-byte path size |
| `orphaned_data.bale` | Valid but not compacted (has gaps from deleted entries) |

### Invalid Archives (`fixtures/invalid/`)

| File | Description |
|------|-------------|
| `bad_crc.bale` | Corrupted CRC-32 checksum |
| `duplicate_paths.bale` | Same path appears multiple times |
| `unsorted_cd.bale` | Central Directory not sorted alphabetically |

## Scripts (`bin/`)

- `run-pjdfstest.sh` - Runs pjdfstest POSIX compliance suite against FUSE mount

## Proptest Regressions (`proptest-regressions/`)

Contains regression test cases discovered by proptest. These are automatically
generated when proptest finds a failing case and should be committed to ensure
the bug stays fixed.

## trycmd Tests (`cmd/`)

The `cmd/` directory contains trycmd test cases as TOML files with expected output:

```text
cmd/
├── check_invalid/     # bale check on invalid archives
├── check_valid/       # bale check on valid archives
├── empty_archive/     # External tool tests (file, zipinfo, unzip)
├── fuse_read/         # FUSE mount read tests
├── single_file/       # External tool tests on single-file archive
├── align_16k/         # zipinfo on 16KB-aligned archive
└── path_2048/         # zipinfo on 2048-byte path archive
```
