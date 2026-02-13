#!/bin/bash
# Quick verification of transient chown behavior.
#
# Build first, then run with sudo:
#   cargo build --release
#   sudo tests/bin/verify-chown.sh

set -e

if [[ $EUID -ne 0 ]]; then
    echo "Error: must run as root (sudo $0)" >&2
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BALE="$PROJECT_DIR/target/release/bale"

if [[ ! -x "$BALE" ]]; then
    echo "Error: $BALE not found. Build first with: cargo build --release" >&2
    exit 1
fi

ARCHIVE="/tmp/test-chown-verify.bale"
rm -f "$ARCHIVE"
"$BALE" touch --path-size 256 "$ARCHIVE"

"$BALE" mount --allow-other "$ARCHIVE" --shell '
echo "=== Create directory ==="
mkdir testdir
stat -c "uid=%u gid=%g mode=%a %n" testdir

echo ""
echo "=== Chown directory to 65534:65534 ==="
chown 65534:65534 testdir
stat -c "uid=%u gid=%g mode=%a %n" testdir

echo ""
echo "=== Stat mount root ==="
stat -c "uid=%u gid=%g mode=%a %n" .

echo ""
echo "=== Create file as nobody inside chowned dir ==="
su -s /bin/sh nobody -c "touch testdir/file.txt" 2>&1 && echo "OK" || echo "FAILED"

echo ""
echo "=== List testdir ==="
ls -la testdir/

echo ""
echo "=== Create file at root as nobody (should fail) ==="
su -s /bin/sh nobody -c "touch rootfile.txt" 2>&1 && echo "OK (unexpected)" || echo "EACCES (expected)"
'

rm -f "$ARCHIVE"
