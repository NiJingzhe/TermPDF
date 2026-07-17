use std::env;
use std::io::Write;
use std::path::PathBuf;

use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum, ValueHint};
use clap_complete::{generate, shells};
use color_eyre::eyre::{OptionExt, Result, bail};

use crate::layout::default_layout_output_dir;
use crate::pdf::PdfBackendOptions;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TermpdfCommand {
    View(PdfBackendOptions),
    Extract(ExtractOptions),
    Grep(GrepOptions),
    Completions(CompletionShell),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum CompletionShell {
    Zsh,
    Fish,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractOptions {
    pub pdf_path: PathBuf,
    pub pdfium_lib_path: Option<PathBuf>,
    pub output_dir: PathBuf,
    pub overwrite: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrepOptions {
    pub layout_dir: PathBuf,
    pub pattern: String,
    pub ignore_case: bool,
    pub literal: bool,
    pub json: bool,
    pub refs_only: bool,
}

#[derive(Parser, Debug)]
#[command(
    name = "termpdf",
    about = "Terminal PDF viewer and layout extractor with kitty image protocol",
    long_about = "Terminal PDF viewer and layout extractor with kitty image protocol.\n\n\
        Subcommands:\n  \
        termpdf <file.pdf>              Open the terminal viewer\n  \
        termpdf extract <file.pdf>     Extract a stable layout pack\n  \
        termpdf grep <pattern> <dir>   Search a layout pack with regex by default\n  \
        termpdf completions <shell>    Generate zsh or fish shell completions\n\n\
        Run `termpdf <subcommand> --help` for subcommand-specific options.",
    after_help = "Viewer keybindings:\n  \
        h, j, k, l           Move text cursor\n  \
        w, b, ^, $           Move by word or line boundary\n  \
        H, J, K, L           Pan viewport\n  \
        Ctrl-u / Ctrl-d      Half-page up/down\n  \
        Ctrl-b / Ctrl-f      Full-page back/forward\n  \
        gg / {count}gg / G  Jump to page\n  \
        /, n, N, Esc         Search, navigate, hide highlight\n  \
        f / F                Follow visible links\n  \
        Tab / Shift-Tab      Focus next/previous PDF image\n  \
        y                    Copy focused image as PNG\n  \
        v / V / Ctrl-v / y  Select text and copy to clipboard\n  \
        m<char> / `<char>    Set and jump to marks\n  \
        F5                   Presentation mode\n  \
        = / - / 0            Zoom in / out / reset\n  \
        i                    Toggle dark mode\n  \
        q                    Quit\n\n\
        Examples:\n  \
        termpdf paper.pdf                                  Open paper.pdf in the viewer\n  \
        termpdf paper.pdf --watch                          Reopen the PDF when the file changes\n  \
        termpdf paper.pdf --dark                           Start in dark mode\n  \
        termpdf paper.pdf --pdfium-lib /opt/pdfium         Use a specific PDFium library\n  \
        termpdf extract paper.pdf                          Write paper.layout/ next to the PDF\n  \
        termpdf extract paper.pdf --out out.layout         Write to a custom layout directory\n  \
        termpdf extract paper.pdf --overwrite              Replace an existing layout pack\n  \
        termpdf grep \"method\" paper.layout                 Print matching refs and text\n  \
        termpdf grep \"method|approach\" paper.layout --json  Output JSON results\n  \
        termpdf grep \"foo.bar\" paper.layout --literal       Treat the pattern as literal text\n  \
        termpdf completions zsh                             Generate zsh completions"
)]
struct CliOptions {
    #[command(subcommand)]
    command: Option<CliSubcommand>,

    #[arg(
        value_name = "FILE",
        value_hint = ValueHint::FilePath,
        help = "PDF file to open in the terminal viewer"
    )]
    pdf_path: Option<PathBuf>,

    #[arg(
        short = 'w',
        long = "watch",
        help = "Reload the PDF when the file changes"
    )]
    watch_mode: bool,

    #[arg(
        long = "pdfium-lib",
        value_name = "PATH",
        value_hint = ValueHint::AnyPath,
        help = "Path to a PDFium dynamic library or directory"
    )]
    pdfium_lib_path: Option<PathBuf>,

    #[arg(long = "dark", help = "Start the terminal viewer in dark mode")]
    dark_mode: bool,
}

#[derive(Subcommand, Debug)]
enum CliSubcommand {
    /// Extract a stable layout pack for agents and LLMs.
    #[command(
        long_about = "Extract a stable layout pack for agents and LLMs.\n\n\
            Each layout pack contains manifest.json, JSONL metadata for pages, text, glyphs, \
            images, and refs, plus text.txt and processed PNG assets. Stable one-based refs include p1, \
            p1.t1, p1.t1.c1, p1.link1, and p1.image1.",
        after_help = "Examples:\n  \
            termpdf extract paper.pdf                         Write paper.layout/ next to the PDF\n  \
            termpdf extract paper.pdf --out out.layout        Write to a custom directory\n  \
            termpdf extract paper.pdf --overwrite             Replace an existing layout pack\n  \
            termpdf extract paper.pdf --pdfium-lib /opt/lib   Use a specific PDFium library"
    )]
    Extract(ExtractCliOptions),
    /// Search text lines in a layout pack and return stable refs.
    #[command(
        long_about = "Search text lines in a layout pack and return stable refs.\n\n\
            By default the PATTERN is treated as a regular expression and the output is \
            `ref<TAB>text` per match. Use --literal for plain text matching, --ignore-case for \
            case-insensitive search, --json for structured JSON, or --refs-only for just the ref.",
        after_help = "Examples:\n  \
            termpdf grep \"method\" paper.layout                       Print matching refs and text\n  \
            termpdf grep \"method|approach\" paper.layout --json        Output JSON results\n  \
            termpdf grep \"foo.bar\" paper.layout --literal             Treat the pattern as literal text\n  \
            termpdf grep \"summary\" paper.layout --refs-only           Print only the matching refs\n  \
            termpdf grep \"term\" paper.layout --ignore-case            Search case-insensitively"
    )]
    Grep(GrepCliOptions),
    /// Generate a shell completion script on stdout.
    #[command(
        long_about = "Generate a shell completion script on stdout.\n\n\
            Supported shells are zsh and fish. Redirect the output to the completion directory \
            used by your shell.",
        after_help = "Examples:\n  \
            termpdf completions zsh > ~/.zfunc/_termpdf\n  \
            termpdf completions fish > ~/.config/fish/completions/termpdf.fish"
    )]
    Completions(CompletionsCliOptions),
}

#[derive(Args, Debug)]
struct ExtractCliOptions {
    #[arg(
        value_name = "FILE",
        value_hint = ValueHint::FilePath,
        help = "PDF file to extract into a layout pack"
    )]
    pdf_path: PathBuf,

    #[arg(
        long = "out",
        value_name = "DIR",
        value_hint = ValueHint::DirPath,
        help = "Output layout pack directory"
    )]
    output_dir: Option<PathBuf>,

    #[arg(long = "overwrite", help = "Replace an existing TermPDF layout pack")]
    overwrite: bool,

    #[arg(
        long = "pdfium-lib",
        value_name = "PATH",
        value_hint = ValueHint::AnyPath,
        help = "Path to a PDFium dynamic library or directory"
    )]
    pdfium_lib_path: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct GrepCliOptions {
    #[arg(
        value_name = "PATTERN",
        value_hint = ValueHint::Other,
        help = "Regex pattern to search for"
    )]
    pattern: String,

    #[arg(
        value_name = "LAYOUT_DIR",
        value_hint = ValueHint::DirPath,
        help = "TermPDF layout pack directory"
    )]
    layout_dir: PathBuf,

    #[arg(short = 'i', long = "ignore-case", help = "Search case-insensitively")]
    ignore_case: bool,

    #[arg(
        long = "literal",
        help = "Interpret PATTERN as literal text instead of a regular expression"
    )]
    literal: bool,

    #[arg(long = "json", help = "Print structured JSON results")]
    json: bool,

    #[arg(long = "refs-only", help = "Print only matching refs")]
    refs_only: bool,
}

#[derive(Args, Debug)]
struct CompletionsCliOptions {
    #[arg(value_enum, value_name = "SHELL", help = "Shell to generate for")]
    shell: CompletionShell,
}

pub fn write_shell_completions<W: Write>(shell: CompletionShell, writer: &mut W) {
    let mut command = CliOptions::command();
    match shell {
        CompletionShell::Zsh => generate(shells::Zsh, &mut command, "termpdf", writer),
        CompletionShell::Fish => generate(shells::Fish, &mut command, "termpdf", writer),
    }
}

impl TermpdfCommand {
    pub fn from_args() -> Result<Self> {
        Self::from_cli(
            CliOptions::parse(),
            env::var_os("PDFIUM_LIB_PATH").map(PathBuf::from),
        )
    }

    pub fn parse_for_tests<I, T>(args: I, default_pdfium_lib_path: Option<PathBuf>) -> Result<Self>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        Self::from_cli(CliOptions::try_parse_from(args)?, default_pdfium_lib_path)
    }

    fn from_cli(cli: CliOptions, env_pdfium_lib_path: Option<PathBuf>) -> Result<Self> {
        let explicit_parent_pdfium_lib_path = cli.pdfium_lib_path.is_some();
        let parent_pdfium_lib_path = cli.pdfium_lib_path;
        match cli.command {
            Some(CliSubcommand::Extract(extract)) => {
                if cli.pdf_path.is_some() {
                    bail!("extract does not accept an extra viewer FILE argument");
                }
                if cli.watch_mode {
                    bail!("--watch is only valid when opening the terminal viewer");
                }
                if cli.dark_mode {
                    bail!("--dark is only valid when opening the terminal viewer");
                }

                Ok(Self::Extract(ExtractOptions {
                    output_dir: extract
                        .output_dir
                        .unwrap_or_else(|| default_layout_output_dir(&extract.pdf_path)),
                    pdf_path: extract.pdf_path,
                    pdfium_lib_path: extract
                        .pdfium_lib_path
                        .or(parent_pdfium_lib_path)
                        .or(env_pdfium_lib_path),
                    overwrite: extract.overwrite,
                }))
            }
            Some(CliSubcommand::Grep(grep)) => {
                if cli.pdf_path.is_some() {
                    bail!("grep does not accept an extra viewer FILE argument");
                }
                if cli.watch_mode {
                    bail!("--watch is only valid when opening the terminal viewer");
                }
                if cli.dark_mode {
                    bail!("--dark is only valid when opening the terminal viewer");
                }
                if explicit_parent_pdfium_lib_path {
                    bail!("--pdfium-lib is only valid for commands that open a PDF");
                }
                if grep.json && grep.refs_only {
                    bail!("--json and --refs-only cannot be used together");
                }

                Ok(Self::Grep(GrepOptions {
                    layout_dir: grep.layout_dir,
                    pattern: grep.pattern,
                    ignore_case: grep.ignore_case,
                    literal: grep.literal,
                    json: grep.json,
                    refs_only: grep.refs_only,
                }))
            }
            Some(CliSubcommand::Completions(completions)) => {
                if cli.pdf_path.is_some() {
                    bail!("completions does not accept an extra viewer FILE argument");
                }
                if cli.watch_mode || cli.dark_mode || explicit_parent_pdfium_lib_path {
                    bail!("viewer options are not valid when generating completions");
                }

                Ok(Self::Completions(completions.shell))
            }
            None => Ok(Self::View(PdfBackendOptions {
                pdf_path: cli.pdf_path.ok_or_eyre(
                    "usage: termpdf <file.pdf> [-w|--watch] [--pdfium-lib /path/to/libpdfium-directory]",
                )?,
                watch_mode: cli.watch_mode,
                pdfium_lib_path: parent_pdfium_lib_path.or(env_pdfium_lib_path),
                dark_mode: cli.dark_mode,
            })),
        }
    }
}
