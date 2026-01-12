#!/bin/bash
# Run pjdfstest against a fresh bale mount, grouped by support status.

set -e

ARCHIVE="/tmp/test-pjdfstest.bale"
TESTS_DIR="$HOME/projects/ref/pjdfstest/tests"
OUTPUT_DIR="$HOME/projects/pjdfstest-results"

# Tests that should work without permission/ownership complications.
# These don't test chown/mkfifo/mknod/link and don't run as different users.
PURE=(
    # ftruncate tests (01-14, excluding 00 which has -u 65534 tests)
    ftruncate/01.t
    ftruncate/04.t
    ftruncate/07.t
    ftruncate/08.t
    ftruncate/09.t
    ftruncate/10.t
    ftruncate/11.t
    ftruncate/12.t
    ftruncate/13.t
    ftruncate/14.t
    # truncate tests (01-14, excluding 00 which has -u 65534 tests)
    truncate/01.t
    truncate/04.t
    truncate/07.t
    truncate/08.t
    truncate/09.t
    truncate/10.t
    truncate/11.t
    truncate/12.t
    truncate/13.t
    truncate/14.t
)

# Tests that have permission enforcement checks (-u 65534 tests).
# We don't implement Unix permission enforcement.
PERM_TESTS=(
    ftruncate/00.t  # Contains -u 65534 (nobody user) permission tests
    truncate/00.t   # Contains -u 65534 (nobody user) permission tests
)

# Tests for supported features but that use unsupported operations
# (chown/mkfifo/mknod/link) in setup, or test permission enforcement.
MIXED=(
    chmod
    mkdir
    open
    rename
    rmdir
    symlink
    unlink
    utimensat
)

# Tests for features we DON'T support (failures expected).
UNSUPPORTED=(
    chown      # Returns EPERM - no Unix ownership in ZIP
    link       # Hard links not supported
    mkfifo     # FIFOs not supported (ENOSYS)
    mknod      # Device nodes not supported (ENOSYS)
    chflags    # BSD-specific, skipped on Linux
    posix_fallocate  # Not implemented
)

mkdir -p "$OUTPUT_DIR"

echo "Running pjdfstest suite..."
echo "Results will be in: $OUTPUT_DIR"
echo ""

run_tests() {
    local name="$1"
    local output="$2"
    shift 2
    local paths="$*"

    echo "=== Running $name tests ==="
    rm -f "$ARCHIVE"
    cargo run --quiet -- touch "$ARCHIVE"
    cargo run --quiet -- mount "$ARCHIVE" --shell \
        "prove -v $paths :: --failures > $output 2>&1 || true"
    echo "  -> $output"
}

# Build test paths for pure tests (specific files).
PURE_PATHS=""
for test in "${PURE[@]}"; do
    if [[ -f "$TESTS_DIR/$test" ]]; then
        PURE_PATHS="$PURE_PATHS $TESTS_DIR/$test"
    fi
done

# Build test paths for permission tests (specific files).
PERM_PATHS=""
for test in "${PERM_TESTS[@]}"; do
    if [[ -f "$TESTS_DIR/$test" ]]; then
        PERM_PATHS="$PERM_PATHS $TESTS_DIR/$test"
    fi
done

# Build test paths for mixed tests (directories).
MIXED_PATHS=""
for group in "${MIXED[@]}"; do
    if [[ -d "$TESTS_DIR/$group" ]]; then
        MIXED_PATHS="$MIXED_PATHS $TESTS_DIR/$group"
    fi
done

# Build test paths for unsupported tests (directories).
UNSUPPORTED_PATHS=""
for group in "${UNSUPPORTED[@]}"; do
    if [[ -d "$TESTS_DIR/$group" ]]; then
        UNSUPPORTED_PATHS="$UNSUPPORTED_PATHS $TESTS_DIR/$group"
    fi
done

# Run each group.
if [[ -n "$PURE_PATHS" ]]; then
    run_tests "PURE (no chown/mkfifo/perm checks)" "$OUTPUT_DIR/pure.txt" $PURE_PATHS
fi

if [[ -n "$PERM_PATHS" ]]; then
    run_tests "PERM (permission enforcement - expect failures)" "$OUTPUT_DIR/perm.txt" $PERM_PATHS
fi

run_tests "MIXED (supported ops, may use unsupported setup)" "$OUTPUT_DIR/mixed.txt" $MIXED_PATHS
run_tests "UNSUPPORTED (expect failures)" "$OUTPUT_DIR/unsupported.txt" $UNSUPPORTED_PATHS

echo ""
echo "=== Summary ==="

if [[ -f "$OUTPUT_DIR/pure.txt" ]]; then
    echo "Pure tests (should all pass):"
    grep -E "^Files=|Result:" "$OUTPUT_DIR/pure.txt" | tail -2 || true
    echo ""
fi

if [[ -f "$OUTPUT_DIR/perm.txt" ]]; then
    echo "Permission tests (expect failures - no perm enforcement):"
    grep -E "^Files=|Result:" "$OUTPUT_DIR/perm.txt" | tail -2 || true
    echo ""
fi

echo "Mixed tests (supported ops, may fail due to setup):"
grep -E "^Files=|Result:" "$OUTPUT_DIR/mixed.txt" | tail -2 || true
echo ""

echo "Unsupported tests (expect failures):"
grep -E "^Files=|Result:" "$OUTPUT_DIR/unsupported.txt" | tail -2 || true
