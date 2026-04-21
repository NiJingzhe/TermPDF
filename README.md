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
  - `armv7-unknown-linux-gnueabihf`
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
- A supported PDFium bundle variant from this repository, or another compatible PDFium dynamic library

For local development, the recommended path is:

```bash
cp termpdf.dev.toml.example termpdf.dev.toml
```

Then set the local PDFium bundle variant and build:

```toml
pdfium_variant = "linux-x64-glibc"
```

```bash
cargo build --release
./target/release/termpdf path/to/file.pdf
```

For packaging and CI, prefer setting the build-time variant directly instead of creating a local config file:

```bash
TERMPDF_PDFIUM_VARIANT=linux-x64-glibc cargo build --release
```

Supported source-build bundle variants:

- `macos-arm64`
- `linux-x64-glibc`
- `linux-arm-glibc`
- `linux-arm64-glibc`

After a successful release build, `build.rs` copies the matching `libpdfium` next to the binary in `target/release/`.

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

For the best developer experience, create a local project config first so every `cargo run` and `cargo build` automatically uses the PDFium bundle for your platform:

```bash
cp termpdf.dev.toml.example termpdf.dev.toml
```

Then edit `termpdf.dev.toml` and set `pdfium_variant` to the bundle that matches your development machine.

```bash
cargo run -- path/to/file.pdf
```

Watch mode:

```bash
cargo run -- path/to/file.pdf -w
```

If PDFium is not available in the system library path, TermPDF will try the bundled binaries under `vendor/pdfium/`. You can also point to a PDFium build explicitly:

```bash
cargo run -- path/to/file.pdf --pdfium-lib /path/to/pdfium
```

## Developer Config

`termpdf.dev.toml` is the project-local development config that selects which vendored PDFium dynamic library Cargo should copy next to the built binary.

Supported values:

- `macos-arm64`
- `linux-x64-glibc`
- `linux-arm-glibc`
- `linux-arm64-glibc`

Example local config:

```toml
pdfium_variant = "linux-x64-glibc"
```

`termpdf.dev.toml` is ignored on purpose, so each developer can choose the right platform locally without changing the repository.

## Bundled PDFium Variants

The repository vendors PDFium `149.0.7789.0` binaries for:

- `macos-arm64`
- `linux-x64-glibc`
- `linux-arm-glibc`
- `linux-arm64-glibc`

When `pdfium_variant` is set in `termpdf.dev.toml`, `build.rs` copies the matching `libpdfium` into `target/<profile>/`, so both `cargo run` and the final executable can load the packaged dynamic library from the binary directory.

The older environment variable and Cargo feature based bundle selection still work, but the recommended path for development is the root-level local config above.

To refresh the vendored PDFium archives from upstream, run:

```bash
./scripts/fetch_pdfium.sh
```

## Releases

Tagged releases build artifacts for the currently supported packaged targets:

- `aarch64-apple-darwin`
- `x86_64-unknown-linux-gnu`
- `armv7-unknown-linux-gnueabihf`
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
