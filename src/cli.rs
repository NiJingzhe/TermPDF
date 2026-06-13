use std::env;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use color_eyre::eyre::{OptionExt, Result, bail};

use crate::layout::default_layout_output_dir;
use crate::pdf::PdfBackendOptions;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TermpdfCommand {
    View(PdfBackendOptions),
    Extract(ExtractOptions),
    Grep(GrepOptions),
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
    pub regex_mode: bool,
    pub json: bool,
    pub refs_only: bool,
}

#[derive(Parser, Debug)]
#[command(
    name = "termpdf",
    about = "Terminal PDF viewer and layout extractor with kitty image protocol",
    after_help = "Keybindings:\n  hjkl                Pan viewport\n  Ctrl-u / Ctrl-d     Half-page up/down\n  Ctrl-b / Ctrl-f     Full-page back/forward\n  gg / {count}gg / G  Jump to page\n  /, n, N, Esc        Search, navigate, hide highlight\n  f / F               Follow visible links\n  m<char> / `<char>   Set and jump to marks\n  F5                  Presentation mode\n  = / - / 0           Zoom in / out / reset\n  i                   Toggle dark mode\n  q                   Quit"
)]
struct CliOptions {
    #[command(subcommand)]
    command: Option<CliSubcommand>,

    #[arg(value_name = "FILE", help = "PDF file to open in the terminal viewer")]
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
        help = "Path to a PDFium dynamic library or directory"
    )]
    pdfium_lib_path: Option<PathBuf>,

    #[arg(long = "dark", help = "Start the terminal viewer in dark mode")]
    dark_mode: bool,
}

#[derive(Subcommand, Debug)]
enum CliSubcommand {
    /// Extract a stable layout pack for agents and LLMs.
    Extract(ExtractCliOptions),
    /// Search text lines in a layout pack and return stable refs.
    Grep(GrepCliOptions),
}

#[derive(Args, Debug)]
struct ExtractCliOptions {
    #[arg(value_name = "FILE", help = "PDF file to extract into a layout pack")]
    pdf_path: PathBuf,

    #[arg(
        long = "out",
        value_name = "DIR",
        help = "Output layout pack directory"
    )]
    output_dir: Option<PathBuf>,

    #[arg(long = "overwrite", help = "Replace an existing TermPDF layout pack")]
    overwrite: bool,

    #[arg(
        long = "pdfium-lib",
        value_name = "PATH",
        help = "Path to a PDFium dynamic library or directory"
    )]
    pdfium_lib_path: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct GrepCliOptions {
    #[arg(
        value_name = "PATTERN",
        help = "Literal text or regex pattern to search for"
    )]
    pattern: String,

    #[arg(value_name = "LAYOUT_DIR", help = "TermPDF layout pack directory")]
    layout_dir: PathBuf,

    #[arg(short = 'i', long = "ignore-case", help = "Search case-insensitively")]
    ignore_case: bool,

    #[arg(
        short = 'E',
        long = "regex",
        help = "Interpret PATTERN as a regular expression"
    )]
    regex_mode: bool,

    #[arg(long = "json", help = "Print structured JSON results")]
    json: bool,

    #[arg(long = "refs-only", help = "Print only matching refs")]
    refs_only: bool,
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
                    regex_mode: grep.regex_mode,
                    json: grep.json,
                    refs_only: grep.refs_only,
                }))
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
