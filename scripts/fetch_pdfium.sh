#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 1 ]]; then
  printf 'usage: %s <pdfium-variant>\n' "$0" >&2
  exit 1
fi

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RELEASE_TAG="chromium/7789"
VARIANT="$1"

case "$VARIANT" in
  macos-arm64)
    STEM="mac-arm64"
    ;;
  linux-x64-glibc)
    STEM="linux-x64"
    ;;
  linux-arm64-glibc)
    STEM="linux-arm64"
    ;;
  *)
    printf 'unsupported pdfium variant: %s\n' "$VARIANT" >&2
    exit 1
    ;;
esac

ARCHIVE_NAME="pdfium-${STEM}.tgz"

CACHE_DIR="$ROOT_DIR/.cache/pdfium/${RELEASE_TAG//\//-}/${VARIANT}"
ARCHIVE_DIR="$(dirname "$CACHE_DIR")"
ARCHIVE_PATH="$ARCHIVE_DIR/$ARCHIVE_NAME"

mkdir -p "$ARCHIVE_DIR"

if [[ ! -f "$ARCHIVE_PATH" ]]; then
  if ! gh release download "$RELEASE_TAG" -R bblanchon/pdfium-binaries -p "$ARCHIVE_NAME" -O "$ARCHIVE_PATH"; then
    curl -L --fail -o "$ARCHIVE_PATH" "https://github.com/bblanchon/pdfium-binaries/releases/download/$RELEASE_TAG/$ARCHIVE_NAME"
  fi
fi

rm -rf "$CACHE_DIR"
mkdir -p "$CACHE_DIR"
tar -xzf "$ARCHIVE_PATH" -C "$CACHE_DIR"

printf '%s\n' "$CACHE_DIR"
