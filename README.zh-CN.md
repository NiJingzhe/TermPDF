# TermPDF

> [English README](README.md)

TermPDF 是一个终端 PDF 阅读器，使用 Rust、ratatui、PDFium 和 Kitty 图形协议构建。

它专注于在兼容 Kitty 图形协议的终端里提供面向阅读的导航体验，通过图像方式渲染 PDF，而不是对文本重新排版。

## 更新日志

### 未发布

- 新增普通模式下的可见 block 光标，支持 Vim 风格文本光标移动（`h`、`j`、`k`、`l`、`w`、`b`、`^`、`$`）及 count 支持。
- 新增 visual 字符选择（`v`）、visual 行选择（`V`）和剪贴板复制（`y`），以纯文本复制到系统剪贴板，使用平台剪贴板命令（`pbcopy`、`wl-copy`、`xclip`、`xsel`、`clip`）。
- 改进 PDF 行聚类算法，使用 glyph 中心线和垂直重叠判断同一行，并增加二阶段合并，将小型内联标注（上标、下标、脚注标记）合并回源顺序相邻的正文行，避免产生多余的单 glyph 行。
- `termpdf grep` 默认改为正则表达式搜索；使用 `--literal` 进行普通文本匹配。

### 0.2.0

- 新增通过 Kitty 图形透传实现的 tmux 支持。在 `~/.tmux.conf` 中启用 `set -g allow-passthrough on` 即可使用。
- 新增鼠标和触控板导航，支持滚动、横向滚动、按住 `Ctrl` 滚轮缩放，以及演示模式导航。

## 功能

- 基于 PDFium 的 PDF 渲染
- 使用 Kitty 图形协议渲染页面图像
- 跨多页文档的平滑滚动
- Vim 风格导航和页码跳转
- 带图像级高亮的搜索
- 使用图像级标签覆盖层打开链接
- 用标记快速导航
- 演示模式
- 深色模式切换
- 监听模式，支持 PDF 实时重载
- 面向 Agent 和 LLM 的 layout pack 抽取，并提供稳定 refs
- Vim 风格 visual 选择，并以纯文本复制到剪贴板

## 安装

### Arch Linux AUR

```bash
yay -S tpdf
```

### macOS

Homebrew tap：

```bash
brew tap NiJingzhe/termpdf
brew install termpdf
```

也可以直接安装：

```bash
brew install NiJingzhe/termpdf/termpdf
```

## 手动安装

### 运行时要求

- 支持的发布平台：
  - `aarch64-apple-darwin`
  - `x86_64-unknown-linux-gnu`
  - `aarch64-unknown-linux-gnu`
- 支持 Kitty 图形协议的终端，例如 kitty 或 ghostty
- 当外层终端支持 Kitty 图形协议，并且在 `~/.tmux.conf` 中启用 `set -g allow-passthrough on` 时支持 tmux；TermPDF 会自动把 Kitty 图像命令包裹在 tmux 透传序列中
- `termpdf` 和匹配的打包版 `libpdfium` 需要放在同一目录，除非你通过 `PDFIUM_LIB_PATH` 或 `--pdfium-lib` 显式指定另一个 PDFium 构建

### 从发布压缩包安装

从 GitHub Releases 页面下载适合你平台的压缩包，然后解压：

```bash
tar -xzf termpdf-0.3.0-x86_64-unknown-linux-gnu.tar.gz
cd termpdf-0.3.0-x86_64-unknown-linux-gnu
./termpdf path/to/file.pdf
```

每个发布压缩包包含：

- `termpdf`
- 匹配的打包版 `libpdfium`
- `LICENSE`
- `README.md`

如果你手动把文件安装到系统目录，请把 `termpdf` 和 `libpdfium.so` 或 `libpdfium.dylib` 保持在同一目录。

### 从源码构建

构建依赖：

- 带 `cargo` 的 Rust stable 工具链
- 使用打包版 PDFium 变体时需要 `gh`，或 `curl` 和 `tar`（`TERMPDF_PDFIUM_VARIANT=SYSTEM` 不需要）
- 一个受支持的 PDFium 打包变体，或另一个兼容的 PDFium 动态库

通过环境变量设置 PDFium 变体并构建：

```bash
TERMPDF_PDFIUM_VARIANT=linux-x64-glibc cargo build --release
./target/release/termpdf path/to/file.pdf
```

`TERMPDF_PDFIUM_VARIANT` 支持的源码构建取值：

- `macos-arm64`
- `linux-x64-glibc`
- `linux-arm64-glibc`
- `SYSTEM`

使用打包变体时，TermPDF 会自动从 `bblanchon/pdfium-binaries` 下载匹配的 PDFium 压缩包到 `.cache/pdfium/`，解压后把匹配的 `libpdfium` 复制到 `target/<profile>/` 中的二进制旁边。

当设置为 `SYSTEM` 时，构建脚本会跳过 PDFium 下载和复制，并在运行时使用系统 PDFium（或通过 `PDFIUM_LIB_PATH` / `--pdfium-lib` 提供的路径）。

### 打包说明

二进制会优先在自身旁边查找打包的 PDFium 库。手动打包、发行版打包或 AUR 打包时，请把真实可执行文件和匹配的 PDFium 库安装到同一目录。

一个可用的 Linux 布局如下：

```bash
/usr/lib/termpdf/termpdf
/usr/lib/termpdf/libpdfium.so
/usr/bin/termpdf
```

其中 `/usr/bin/termpdf` 是一个很小的包装脚本：

```bash
#!/usr/bin/env bash
exec /usr/lib/termpdf/termpdf "$@"
```

对于 `x86_64` 上的 AUR source package，构建步骤应等价于：

```bash
TERMPDF_PDFIUM_VARIANT=linux-x64-glibc cargo build --release
```

对于 `x86_64` 上的 AUR binary package，请解压发布 tarball，并把其中打包好的 `termpdf` 和 `libpdfium.so` 一起安装，不要把它们拆分到不同目录。

## 使用

源码构建时，将 `TERMPDF_PDFIUM_VARIANT` 设置为与你机器匹配的打包变体，或设置为 `SYSTEM` 以使用系统 PDFium。

```bash
TERMPDF_PDFIUM_VARIANT=linux-x64-glibc cargo run -- path/to/file.pdf
```

监听模式：

```bash
TERMPDF_PDFIUM_VARIANT=linux-x64-glibc cargo run -- path/to/file.pdf -w
```

如果 PDFium 不在系统库路径中，TermPDF 会尝试使用 `.cache/pdfium/` 下已下载的缓存。你也可以显式指定 PDFium 构建路径：

```bash
cargo run -- path/to/file.pdf --pdfium-lib /path/to/pdfium
```

## Layout Pack 抽取

TermPDF 可以为 Agent、LLM、搜索流水线和其他 CLI 工具抽取稳定的 layout pack：

```bash
termpdf extract path/to/file.pdf --out path/to/file.layout
```

如果省略 `--out`，TermPDF 会在源 PDF 旁边写入 `.layout` 后缀目录：

```bash
termpdf extract paper.pdf
# 写入 paper.layout/
```

使用 `--overwrite` 替换已有的 TermPDF layout pack：

```bash
termpdf extract paper.pdf --overwrite
```

每个 layout pack 包含：

- `manifest.json`：schema、TermPDF 版本、源 PDF hash、坐标系统和文件映射
- `pages.jsonl`：每页一条记录
- `blocks.jsonl`：文本行和链接记录
- `glyphs.jsonl`：每个可见字符一条精确 glyph 记录
- `refs.jsonl`：用于快速查询的全局引用注册表

稳定 refs 使用一基编号和类型命名空间：

```text
p1           第 1 页
p1.t1        第 1 页第 1 条文本行
p1.t1.c1     第 1 页第 1 条文本行的第 1 个字符
p1.link1     第 1 页第 1 个链接
```

layout schema 为 `termpdf.layout.v1`。bbox 使用 PDF points，原点在左下角，与 PDFium 抽取和 TermPDF 渲染投影保持一致。

使用 `grep` 搜索 layout pack 并返回稳定 refs：

```bash
termpdf grep "method" paper.layout
termpdf grep "method" paper.layout --refs-only
termpdf grep "method|approach" paper.layout --json
termpdf grep "literal.dot" paper.layout --literal
```

默认情况下，`grep` 会把 pattern 当作正则表达式，并输出 `ref<TAB>text`。使用 `--ignore-case` 进行大小写不敏感搜索；使用 `--literal` 时，pattern 会按普通文本解释。

## 构建环境

`TERMPDF_PDFIUM_VARIANT` 用来选择 Cargo 应该下载哪一个 PDFium 动态库，并把它复制到构建出的二进制旁边。

支持的取值：

- `macos-arm64`
- `linux-x64-glibc`
- `linux-arm64-glibc`
- `SYSTEM`（跳过下载和复制；使用系统 PDFium）

示例：

```bash
TERMPDF_PDFIUM_VARIANT=linux-x64-glibc cargo build --release
```

`TERMPDF_PDFIUM_VARIANT` 是开发、打包和 CI 的推荐方式，因为它让构建配置显式，并且只作用于当前命令。

## 打包版 PDFium 变体

当前构建支持自动下载这些 PDFium：

- `macos-arm64`
- `linux-x64-glibc`
- `linux-arm64-glibc`

当 `TERMPDF_PDFIUM_VARIANT` 设置为上述某个打包变体时，`build.rs` 会按需下载匹配的 PDFium 压缩包，缓存到 `.cache/pdfium/`，并把匹配的 `libpdfium` 复制到 `target/<profile>/`，这样 `cargo run` 和最终可执行文件都可以从二进制目录加载打包的动态库。

当 `TERMPDF_PDFIUM_VARIANT=SYSTEM` 时，`build.rs` 会跳过打包流程，并依赖运行时的系统 PDFium 解析。

旧的基于 Cargo feature 的打包选择仍然可用，但开发时推荐使用上面的环境变量。

如需从上游刷新 vendored PDFium 压缩包，运行：

```bash
./scripts/fetch_pdfium.sh linux-x64-glibc
```

## 发布

打标签发布会为当前支持的打包目标构建产物：

- `aarch64-apple-darwin`
- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`

每个发布压缩包包含：

- `termpdf`
- 匹配的打包版 `libpdfium`
- `LICENSE`
- `README.md`

## 快捷键

- `h` / `j` / `k` / `l`：移动文本光标
- `w` / `b` / `^` / `$`：按词或行边界移动光标
- `H` / `J` / `K` / `L`：平移视口
- `Ctrl-u` / `Ctrl-d`：向上/向下半页
- `Ctrl-b` / `Ctrl-f`：向前/向后一整页
- `gg`、`{count}gg`、`G`：跳转到页面
- `/`、`n`、`N`、`Esc`：搜索、浏览结果、隐藏高亮
- `f` / `F`：打开可见链接
- `v`：普通 visual 字符选择
- `V`：visual 行选择
- `y`：把当前 visual 选择以纯文本复制到系统剪贴板
- `m<char>` / `` `<char> ``：设置标记并跳转到标记
- `F5`：演示模式
- `=` / `-` / `0`：放大、缩小、重置缩放
- `i`：切换深色模式
- `q`：退出

## 状态

本项目正在积极开发中。
