use crossterm::event::{KeyCode, KeyEvent};
use termpdf::app::{App, RunOptions, run};
use termpdf::cli::{ExtractOptions, GrepOptions, TermpdfCommand};
use termpdf::layout::{
    LayoutGrepOptions, LayoutPack, LayoutWriteOptions, SourceMetadata, grep_layout_pack,
};
use termpdf::pdf::{PdfBackend, PdfBackendOptions};

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    match TermpdfCommand::from_args()? {
        TermpdfCommand::View(options) => run_view(options),
        TermpdfCommand::Extract(options) => run_extract(options),
        TermpdfCommand::Grep(options) => run_grep(options),
    }
}

fn run_view(options: PdfBackendOptions) -> color_eyre::Result<()> {
    let backend = PdfBackend::new(options.pdfium_lib_path.as_deref())?;
    let mut session = backend.open_session(&options.pdf_path)?;
    let document = session.document().clone();
    let mut app = App::with_path(document, session.pdf_path().to_path_buf());
    if options.dark_mode {
        app.handle_key(KeyEvent::from(KeyCode::Char('i')));
    }
    let run_options = RunOptions::new(options.watch_mode);
    ratatui::run(|terminal| run(terminal, &mut app, &backend, &mut session, run_options))?;

    Ok(())
}

fn run_extract(options: ExtractOptions) -> color_eyre::Result<()> {
    let backend = PdfBackend::new(options.pdfium_lib_path.as_deref())?;
    let session = backend.open_session(&options.pdf_path)?;
    let source = SourceMetadata::from_path(&options.pdf_path)?;
    let pack = LayoutPack::from_document(session.document(), source);
    let result = pack.write_to_dir(
        &options.output_dir,
        LayoutWriteOptions::new(options.overwrite),
    )?;

    println!("Wrote layout pack to {}", result.output_dir.display());

    Ok(())
}

fn run_grep(options: GrepOptions) -> color_eyre::Result<()> {
    let matches = grep_layout_pack(
        &options.layout_dir,
        &options.pattern,
        LayoutGrepOptions::new(options.ignore_case, options.literal),
    )?;

    if options.json {
        serde_json::to_writer_pretty(std::io::stdout().lock(), &matches)?;
        println!();
        return Ok(());
    }

    for matched in matches {
        if options.refs_only {
            println!("{}", matched.ref_id);
        } else {
            println!("{}\t{}", matched.ref_id, matched.text);
        }
    }

    Ok(())
}
