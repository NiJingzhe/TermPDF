# TermPDF

> **[中文 README 点这里](README.zh-CN.md)**

TermPDF is a terminal PDF reader built with Rust, ratatui, PDFium, and the kitty image protocol.

It focuses on reader-oriented navigation for kitty-compatible terminals, with image-based PDF rendering instead of text reflow.

## CHANGELOG

### Unreleased

- Added a visible block cursor in normal mode with Vim-style text cursor motions (`h`, `j`, `k`, `l`, `w`, `b`, `^`, `$`) and count support.
- Added visual character selection (`v`), visual line selection (`V`), and clipboard copy (`y`) as plain text using platform clipboard commands (`pbcopy`, `wl-copy`, `xclip`, `xsel`, `clip`).
- Improved PDF line clustering to use glyph center lines and vertical overlap, with a second pass that merges small inline annotations (superscripts, subscripts, footnote markers) into their source-adjacent body line instead of creating spurious single-glyph lines.
- Changed `termpdf grep` to default to regular expression search; use `--literal` for plain text matching.

### 0.2.0

- Added tmux support through Kitty graphics passthrough. Enable it in `~/.tmux.conf` with `set -g allow-passthrough on`.
- Added mouse and touchpad navigation for scrolling, horizontal scrolling, zooming with `Ctrl` + scroll, and presentation navigation.

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
- Agent and LLM-oriented layout pack extraction with stable refs
- Vim-style visual selection with clipboard copy as plain text

## Install

### For Arch Linux with AUR

```bash
yay -S tpdf
```

### For Mac:

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
- tmux is supported when the outer terminal supports Kitty graphics and `set -g allow-passthrough on` is enabled in `~/.tmux.conf`; TermPDF wraps Kitty image commands in tmux passthrough automatically
- `termpdf` and the matching packaged `libpdfium` in the same directory, unless you explicitly point to another PDFium build with `PDFIUM_LIB_PATH` or `--pdfium-lib`

### Install From A Release Archive

Download the archive for your platform from the GitHub Releases page, then extract it:

```bash
tar -xzf termpdf-0.2.0-x86_64-unknown-linux-gnu.tar.gz
cd termpdf-0.2.0-x86_64-unknown-linux-gnu
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

## Layout Pack Extraction

TermPDF can extract a stable layout pack for agents, LLMs, search pipelines, and other CLI tools:

```bash
termpdf extract path/to/file.pdf --out path/to/file.layout
```

If `--out` is omitted, TermPDF writes next to the source PDF with a `.layout` suffix:

```bash
termpdf extract paper.pdf
# writes paper.layout/
```

Use `--overwrite` to replace an existing TermPDF layout pack:

```bash
termpdf extract paper.pdf --overwrite
```

Each layout pack contains:

- `manifest.json`: schema, TermPDF version, source PDF hash, coordinate system, and file map
- `pages.jsonl`: one page record per line
- `blocks.jsonl`: text line and link records
- `glyphs.jsonl`: one precise glyph record per visible character
- `refs.jsonl`: a global reference registry for quick lookup

Stable refs use one-based, type-namespaced addresses:

```text
p1           page 1
p1.t1        page 1, text line 1
p1.t1.c1     page 1, text line 1, character 1
p1.link1     page 1, link 1
```

The layout schema is `termpdf.layout.v1`. Bboxes use PDF points with a bottom-left origin, matching PDFium extraction and TermPDF rendering projection.

Search a layout pack and return stable refs with `grep`:

```bash
termpdf grep "method" paper.layout
termpdf grep "method" paper.layout --refs-only
termpdf grep "method|approach" paper.layout --json
termpdf grep "literal.dot" paper.layout --literal
```

By default, `grep` treats the pattern as a regular expression and prints `ref<TAB>text`. Use `--ignore-case` for case-insensitive search and `--literal` when the pattern should be treated as plain text.

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

- `h` / `j` / `k` / `l`: move the text cursor
- `w` / `b` / `^` / `$`: move by word or line boundary
- `H` / `J` / `K` / `L`: pan viewport
- `Ctrl-u` / `Ctrl-d`: half-page up/down
- `Ctrl-b` / `Ctrl-f`: full-page back/forward
- `gg`, `{count}gg`, `G`: jump to page
- `/`, `n`, `N`, `Esc`: search, navigate results, hide highlights
- `f` / `F`: follow visible links
- `v`: visual character selection
- `V`: visual line selection
- `y`: copy the active visual selection to the system clipboard as plain text
- `m<char>` / `` `<char> ``: set and jump to marks
- `F5`: presentation mode
- `=` / `-` / `0`: zoom in / out / reset
- `i`: toggle dark mode
- `q`: quit

## Status

This project is under active development.
