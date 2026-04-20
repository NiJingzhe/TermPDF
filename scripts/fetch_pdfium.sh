#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(dirname "$0")/.."
RELEASE_TAG="chromium/7789"

download_and_extract() {
  local asset_name="$1"
  local vendor_dir="$2"

  mkdir -p "$ROOT_DIR/vendor/pdfium/$vendor_dir"
  gh release download "$RELEASE_TAG" \
    -R bblanchon/pdfium-binaries \
    -p "$asset_name" \
    -D "$ROOT_DIR/vendor/pdfium/$vendor_dir"
  tar -xzf "$ROOT_DIR/vendor/pdfium/$vendor_dir/$asset_name" \
    -C "$ROOT_DIR/vendor/pdfium/$vendor_dir"
}

download_and_extract "pdfium-linux-x64.tgz" "linux-x64-glibc"
download_and_extract "pdfium-linux-x86.tgz" "linux-x86-glibc"
download_and_extract "pdfium-linux-arm.tgz" "linux-arm-glibc"
download_and_extract "pdfium-linux-arm64.tgz" "linux-arm64-glibc"
download_and_extract "pdfium-linux-ppc64.tgz" "linux-ppc64-glibc"
download_and_extract "pdfium-mac-arm64.tgz" "macos-arm64"
