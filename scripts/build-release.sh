#!/usr/bin/env bash
# Build HVOC release binaries and package them for distribution.
#
# Usage:
#   bash scripts/build-release.sh
#
# Output:
#   dist/hvoc-<target>.zip   — binary + frontend + README

set -euo pipefail

echo "=== HVOC Release Build ==="

# Detect target triple
TARGET="${TARGET:-$(rustc -vV | grep '^host:' | awk '{print $2}')}"
echo "Target: $TARGET"

# Build release binary
echo ""
echo "Building release binary..."
cargo build --release --package hvoc-cli

# Determine binary name
if [[ "$TARGET" == *"windows"* ]]; then
  BIN="target/release/hvoc-cli.exe"
else
  BIN="target/release/hvoc-cli"
fi

if [[ ! -f "$BIN" ]]; then
  echo "ERROR: binary not found at $BIN"
  exit 1
fi

SIZE=$(du -h "$BIN" | awk '{print $1}')
echo "Binary built: $BIN ($SIZE)"

# Create distribution directory
DIST_DIR="dist"
STAGE_DIR="$DIST_DIR/hvoc-$TARGET"
rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR"

# Copy files
cp "$BIN" "$STAGE_DIR/"
cp hvoc.html "$STAGE_DIR/"
cp README.md "$STAGE_DIR/"

# Create archive
echo ""
echo "Packaging..."
cd "$DIST_DIR"
if command -v zip >/dev/null 2>&1; then
  zip -r "hvoc-$TARGET.zip" "hvoc-$TARGET/"
  ARCHIVE="dist/hvoc-$TARGET.zip"
elif command -v tar >/dev/null 2>&1; then
  tar czf "hvoc-$TARGET.tar.gz" "hvoc-$TARGET/"
  ARCHIVE="dist/hvoc-$TARGET.tar.gz"
else
  echo "WARNING: neither zip nor tar found; files staged in $STAGE_DIR"
  ARCHIVE="$STAGE_DIR"
fi
cd ..

echo ""
echo "=== Build Complete ==="
echo "Binary:  $BIN"
echo "Package: $ARCHIVE"
echo ""
echo "To run:"
echo "  1. Extract the archive"
echo "  2. Run: ./hvoc-cli serve"
echo "  3. Open hvoc.html in a browser"
