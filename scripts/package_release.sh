#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 4 ]]; then
  printf 'usage: %s <target-triple> <pdfium-variant> <binary-path> <output-dir>\n' "$0" >&2
  exit 1
fi

TARGET_TRIPLE="$1"
PDFIUM_VARIANT="$2"
BINARY_PATH="$3"
OUTPUT_DIR="$4"
VERSION="${TERMPDF_RELEASE_VERSION:-0.1.0}"
ARCHIVE_BASENAME="termpdf-${VERSION}-${TARGET_TRIPLE}"
STAGE_DIR="${OUTPUT_DIR}/${ARCHIVE_BASENAME}"

rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR"

cp "$BINARY_PATH" "$STAGE_DIR/termpdf"
cp "LICENSE" "$STAGE_DIR/LICENSE"
cp "README.md" "$STAGE_DIR/README.md"

case "$PDFIUM_VARIANT" in
  macos-*)
    PDFIUM_LIB_NAME="libpdfium.dylib"
    ;;
  *)
    PDFIUM_LIB_NAME="libpdfium.so"
    ;;
esac

cp "vendor/pdfium/${PDFIUM_VARIANT}/lib/${PDFIUM_LIB_NAME}" "$STAGE_DIR/${PDFIUM_LIB_NAME}"

tar -czf "${OUTPUT_DIR}/${ARCHIVE_BASENAME}.tar.gz" -C "$OUTPUT_DIR" "$ARCHIVE_BASENAME"
