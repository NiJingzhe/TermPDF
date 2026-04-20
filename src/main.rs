use crossterm::event::{KeyCode, KeyEvent};
use termpdf::app::{run, App, RunOptions};
use termpdf::pdf::{PdfBackend, PdfBackendOptions};

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let options = PdfBackendOptions::from_args()?;
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
