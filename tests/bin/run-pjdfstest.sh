#!/bin/bash
# Run pjdfstest against a fresh bale mount, grouped by support status.
#
# Build first (as non-root), then run with sudo:
#   cargo build --release
#   sudo tests/bin/run-pjdfstest.sh
#
# See tests/bin/run-pjdfstest.md for detailed results documentation.

set -e

# Require root for proper permission/ownership testing.
if [[ $EUID -ne 0 ]]; then
    echo "Error: must run as root (sudo $0)" >&2
    exit 1
fi

# Resolve the invoking user's home directory (not root's).
if [[ -n "$SUDO_USER" ]]; then
    USER_HOME="$(getent passwd "$SUDO_USER" | cut -d: -f6)"
else
    USER_HOME="$HOME"
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BALE="$PROJECT_DIR/target/release/bale"

if [[ ! -x "$BALE" ]]; then
    echo "Error: $BALE not found. Build first with: cargo build --release" >&2
    exit 1
fi

ARCHIVE="/tmp/test-pjdfstest.bale"
TESTS_DIR="$USER_HOME/projects/ref/pjdfstest/tests"
OUTPUT_DIR="$USER_HOME/projects/pjdfstest-results"
# Use 4096 path size to match POSIX PATH_MAX for pjdfstest compliance.
PATH_SIZE=4096

# ─── Group 1: PASS ─────────────────────────────────────────────────────
# Tests that should all pass. Any failure here is a regression.
PASS=(
    # ftruncate (excluding 00,05,06 which have perm tests)
    ftruncate/01.t ftruncate/02.t ftruncate/03.t ftruncate/04.t
    ftruncate/07.t ftruncate/08.t ftruncate/09.t ftruncate/10.t
    ftruncate/11.t ftruncate/12.t ftruncate/13.t ftruncate/14.t
    # truncate (excluding 00,05,06 which have perm tests)
    truncate/01.t truncate/02.t truncate/03.t truncate/04.t
    truncate/07.t truncate/08.t truncate/09.t truncate/10.t
    truncate/11.t truncate/12.t truncate/13.t truncate/14.t
    # chmod (excluding 00,01,11 which use mkfifo/mknod/bind)
    chmod/02.t chmod/03.t chmod/04.t chmod/05.t chmod/06.t
    chmod/07.t chmod/08.t chmod/09.t chmod/10.t chmod/12.t
    # link (excluding 00,01,10 which use mkfifo/mknod/bind or perm tests)
    link/02.t link/03.t link/04.t link/05.t link/06.t link/07.t
    link/08.t link/09.t link/11.t link/12.t link/13.t link/14.t
    link/15.t link/16.t link/17.t
    # mkdir (excluding 00,01,10 which use mkfifo/mknod/bind)
    mkdir/02.t mkdir/03.t mkdir/04.t mkdir/05.t mkdir/06.t
    mkdir/07.t mkdir/08.t mkdir/09.t mkdir/11.t mkdir/12.t
    # open (excluding 00,01,06,17,22,24 which use mkfifo/mknod/bind or perm tests)
    open/02.t open/03.t open/04.t open/05.t open/07.t open/08.t
    open/09.t open/10.t open/11.t open/12.t open/13.t open/14.t
    open/15.t open/16.t open/18.t open/19.t open/20.t open/21.t
    open/23.t open/25.t
    # rename (excluding 00,09,10,12,13,14,20,23,24)
    rename/01.t rename/02.t rename/03.t rename/04.t rename/05.t
    rename/06.t rename/07.t rename/08.t rename/11.t rename/15.t
    rename/16.t rename/17.t rename/18.t rename/19.t rename/21.t
    rename/22.t
    # rmdir (excluding 00,01,06 which use mkfifo/mknod/bind)
    rmdir/02.t rmdir/03.t rmdir/04.t rmdir/05.t rmdir/07.t
    rmdir/08.t rmdir/09.t rmdir/10.t rmdir/11.t rmdir/12.t
    rmdir/13.t rmdir/14.t rmdir/15.t
    # symlink (excluding 00,02,08 which have description-less/length/perm failures)
    symlink/01.t symlink/03.t symlink/04.t symlink/05.t
    symlink/06.t symlink/07.t symlink/09.t symlink/10.t
    symlink/11.t symlink/12.t
    # unlink (excluding 00,11,14 which use mkfifo/mknod/bind or open-unlink tests)
    unlink/01.t unlink/02.t unlink/03.t unlink/04.t unlink/05.t
    unlink/06.t unlink/07.t unlink/08.t unlink/09.t unlink/10.t
    unlink/12.t unlink/13.t
    # utimensat (excluding 00,02,04,05,08,09 which have atime/perm/cascade failures)
    utimensat/01.t utimensat/03.t utimensat/06.t utimensat/07.t
)

# ─── Group 2: KNOWN FAILURES ──────────────────────────────────────────
# Tests that fail due to known limitations (unsupported node types in
# setup, missing atime, no open-unlink semantics, permission checks).
# Tracked so improvements can be measured over time.
KNOWN_FAIL=(
    # Permission enforcement tests (-u 65534 / nobody user / chown)
    ftruncate/00.t ftruncate/05.t ftruncate/06.t
    truncate/00.t truncate/05.t truncate/06.t
    # Uses mkfifo/mknod/bind in setup (cascade failures)
    chmod/00.t chmod/01.t chmod/11.t
    link/00.t link/01.t link/10.t
    mkdir/00.t mkdir/01.t mkdir/10.t
    open/00.t open/01.t open/06.t open/17.t open/22.t open/24.t
    rename/00.t rename/09.t rename/10.t rename/12.t rename/13.t
    rename/14.t rename/20.t rename/23.t rename/24.t
    rmdir/00.t rmdir/01.t rmdir/06.t
    symlink/00.t symlink/02.t symlink/08.t
    unlink/00.t unlink/11.t unlink/14.t
    utimensat/00.t utimensat/02.t utimensat/04.t utimensat/05.t
    utimensat/08.t utimensat/09.t
)

# ─── Group 3: UNSUPPORTED ─────────────────────────────────────────────
# Features we don't implement. Entire directories, failures expected.
UNSUPPORTED=(
    chown      # Transient-only, not POSIX-compliant
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
    "$BALE" touch --path-size "$PATH_SIZE" "$ARCHIVE"
    "$BALE" mount --allow-other "$ARCHIVE" --shell \
        "prove -v $paths :: --failures > $output 2>&1 || true"
    echo "  -> $output"
}

# Build test paths for each group.
PASS_PATHS=""
for test in "${PASS[@]}"; do
    [[ -f "$TESTS_DIR/$test" ]] && PASS_PATHS="$PASS_PATHS $TESTS_DIR/$test"
done

KNOWN_FAIL_PATHS=""
for test in "${KNOWN_FAIL[@]}"; do
    [[ -f "$TESTS_DIR/$test" ]] && KNOWN_FAIL_PATHS="$KNOWN_FAIL_PATHS $TESTS_DIR/$test"
done

UNSUPPORTED_PATHS=""
for group in "${UNSUPPORTED[@]}"; do
    [[ -d "$TESTS_DIR/$group" ]] && UNSUPPORTED_PATHS="$UNSUPPORTED_PATHS $TESTS_DIR/$group"
done

# Run each group (PASS first — any failure here is a regression).
if [[ -n "$PASS_PATHS" ]]; then
    run_tests "PASS (should all pass)" "$OUTPUT_DIR/pass.txt" $PASS_PATHS
fi

if [[ -n "$KNOWN_FAIL_PATHS" ]]; then
    run_tests "KNOWN FAILURES (unsupported ops in setup)" "$OUTPUT_DIR/known-fail.txt" $KNOWN_FAIL_PATHS
fi

if [[ -n "$UNSUPPORTED_PATHS" ]]; then
    run_tests "UNSUPPORTED (expect failures)" "$OUTPUT_DIR/unsupported.txt" $UNSUPPORTED_PATHS
fi

echo ""
echo "=== Summary ==="

if [[ -f "$OUTPUT_DIR/pass.txt" ]]; then
    echo "PASS (any failure = regression):"
    grep -E "^Files=|Result:" "$OUTPUT_DIR/pass.txt" | tail -2 || true
    echo ""
fi

if [[ -f "$OUTPUT_DIR/known-fail.txt" ]]; then
    echo "KNOWN FAILURES (tracked for improvement):"
    grep -E "^Files=|Result:" "$OUTPUT_DIR/known-fail.txt" | tail -2 || true
    echo ""
fi

if [[ -f "$OUTPUT_DIR/unsupported.txt" ]]; then
    echo "UNSUPPORTED (failures expected):"
    grep -E "^Files=|Result:" "$OUTPUT_DIR/unsupported.txt" | tail -2 || true
fi
