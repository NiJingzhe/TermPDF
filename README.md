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

## Usage

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
