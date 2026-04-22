# TermPDF

TermPDF is a terminal PDF reader built with Rust, ratatui, PDFium, and the kitty image protocol.

It focuses on reader-oriented navigation for kitty-compatible terminals, with image-based PDF rendering instead of text reflow.

## Features

- PDFium-backed PDF rendering
- Kitty image protocol rendering for page images
- Smooth scrolling across multi-page documents
- Vim-style navigation and page jumps
- Search with image-level highlights
- Follow links with image-level tag overlays
- Marks for quick navigation
- Presentation mode
- Dark mode toggle
- Watch mode with live PDF reload

## Install

Homebrew tap:

```bash
brew tap NiJingzhe/termpdf
brew install termpdf
```

Or install directly:

```bash
brew install NiJingzhe/termpdf/termpdf
```

## Manual Install

### Runtime Requirements

- A supported release platform:
  - `aarch64-apple-darwin`
  - `x86_64-unknown-linux-gnu`
  - `aarch64-unknown-linux-gnu`
- A terminal with kitty graphics protocol support, such as kitty or ghostty
- `termpdf` and the matching packaged `libpdfium` in the same directory, unless you explicitly point to another PDFium build with `PDFIUM_LIB_PATH` or `--pdfium-lib`

### Install From A Release Archive

Download the archive for your platform from the GitHub Releases page, then extract it:

```bash
tar -xzf termpdf-0.1.0-x86_64-unknown-linux-gnu.tar.gz
cd termpdf-0.1.0-x86_64-unknown-linux-gnu
./termpdf path/to/file.pdf
```

Each release archive contains:

- `termpdf`
- the matching packaged `libpdfium`
- `LICENSE`
- `README.md`

If you install the files manually into the filesystem, keep `termpdf` and `libpdfium.so` or `libpdfium.dylib` together in the same directory.

### Build From Source

Build dependencies:

- Rust stable toolchain with `cargo`
- `gh` or `curl` and `tar` when using bundled PDFium variants (not required for `TERMPDF_PDFIUM_VARIANT=SYSTEM`)
- A supported PDFium bundle variant, or another compatible PDFium dynamic library

Set the PDFium variant with an environment variable and build:

```bash
TERMPDF_PDFIUM_VARIANT=linux-x64-glibc cargo build --release
./target/release/termpdf path/to/file.pdf
```

Supported source-build values for `TERMPDF_PDFIUM_VARIANT`:

- `macos-arm64`
- `linux-x64-glibc`
- `linux-arm64-glibc`
- `SYSTEM`

When using a bundled variant, TermPDF automatically downloads the matching PDFium archive from `bblanchon/pdfium-binaries` into `.cache/pdfium/`, extracts it, and then copies the matching `libpdfium` next to the binary in `target/<profile>/`.

When set to `SYSTEM`, the build skips PDFium download/copy in `build.rs` and uses your system PDFium at runtime (or a path provided by `PDFIUM_LIB_PATH` / `--pdfium-lib`).

### Packaging Notes

The binary looks for a packaged PDFium library next to itself first. For manual packaging, distro packaging, or AUR packaging, install the real executable and the matching PDFium library into the same directory.

A working Linux layout is:

```bash
/usr/lib/termpdf/termpdf
/usr/lib/termpdf/libpdfium.so
/usr/bin/termpdf
```

Where `/usr/bin/termpdf` is a small wrapper:

```bash
#!/usr/bin/env bash
exec /usr/lib/termpdf/termpdf "$@"
```

For an AUR source package on `x86_64`, the build step should be equivalent to:

```bash
TERMPDF_PDFIUM_VARIANT=linux-x64-glibc cargo build --release
```

For an AUR binary package on `x86_64`, unpack the release tarball and install the bundled `termpdf` and `libpdfium.so` together without separating them.

## Usage

For source builds, set `TERMPDF_PDFIUM_VARIANT` to the bundle that matches your machine, or set it to `SYSTEM` to use system PDFium.

```bash
TERMPDF_PDFIUM_VARIANT=linux-x64-glibc cargo run -- path/to/file.pdf
```

Watch mode:

```bash
TERMPDF_PDFIUM_VARIANT=linux-x64-glibc cargo run -- path/to/file.pdf -w
```

If PDFium is not available in the system library path, TermPDF will try the downloaded cache under `.cache/pdfium/`. You can also point to a PDFium build explicitly:

```bash
cargo run -- path/to/file.pdf --pdfium-lib /path/to/pdfium
```

## Build Environment

`TERMPDF_PDFIUM_VARIANT` selects which PDFium dynamic library Cargo should download and copy next to the built binary.

Supported values:

- `macos-arm64`
- `linux-x64-glibc`
- `linux-arm64-glibc`
- `SYSTEM` (skip download/copy; use system PDFium)

Example:

```bash
TERMPDF_PDFIUM_VARIANT=linux-x64-glibc cargo build --release
```

`TERMPDF_PDFIUM_VARIANT` is the recommended path for development, packaging, and CI because it keeps the build configuration explicit and local to the command being run.

## Bundled PDFium Variants

The build currently supports automatic PDFium downloads for:

- `macos-arm64`
- `linux-x64-glibc`
- `linux-arm64-glibc`

When `TERMPDF_PDFIUM_VARIANT` is set to one of the bundled variants above, `build.rs` downloads the matching PDFium archive if needed, caches it in `.cache/pdfium/`, and copies the matching `libpdfium` into `target/<profile>/`, so both `cargo run` and the final executable can load the packaged dynamic library from the binary directory.

When `TERMPDF_PDFIUM_VARIANT=SYSTEM`, `build.rs` skips bundling and relies on system PDFium resolution at runtime.

The older Cargo feature based bundle selection still works, but the recommended path for development is the environment variable above.

To refresh the vendored PDFium archives from upstream, run:

```bash
./scripts/fetch_pdfium.sh linux-x64-glibc
```

## Releases

Tagged releases build artifacts for the currently supported packaged targets:

- `aarch64-apple-darwin`
- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`

Each release archive contains:

- `termpdf`
- the matching packaged `libpdfium`
- `LICENSE`
- `README.md`

## Keybindings

- `h j k l`: pan viewport
- `Ctrl-u` / `Ctrl-d`: half-page up/down
- `Ctrl-b` / `Ctrl-f`: full-page back/forward
- `gg`, `{count}gg`, `G`: jump to page
- `/`, `n`, `N`, `Esc`: search, navigate results, hide highlights
- `f` / `F`: follow visible links
- `m<char>` / `` `<char> ``: set and jump to marks
- `F5`: presentation mode
- `=` / `-` / `0`: zoom in / out / reset
- `i`: toggle dark mode
- `q`: quit

## Status

This project is under active development.
