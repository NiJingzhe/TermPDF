use std::path::Path;
use std::sync::{Mutex, OnceLock};

use crossterm::event::{KeyCode, KeyEvent};
use termpdf::app::App;
use termpdf::document::{Document, OutlineNode, Page};
use termpdf::pdf::PdfBackend;

fn outline_document(outline: Vec<OutlineNode>, page_count: usize) -> Document {
    Document {
        pages: (0..page_count)
            .map(|page| Page::from_text(page, &["body line"]))
            .collect(),
        outline,
    }
}

#[test]
fn flattened_outline_lists_entries_in_depth_first_prefix_order() {
    let document = outline_document(
        vec![
            OutlineNode {
                title: "Chapter One".to_string(),
                page: Some(0),
                children: vec![
                    OutlineNode::new("Section 1.1".to_string(), Some(0)),
                    OutlineNode {
                        title: "Section 1.2".to_string(),
                        page: Some(1),
                        children: vec![OutlineNode::new("Subsection 1.2.1".to_string(), Some(1))],
                    },
                ],
            },
            OutlineNode::new("Chapter Two".to_string(), Some(2)),
        ],
        3,
    );

    let entries = document.flattened_outline();

    let rendered: Vec<(usize, &str, Option<usize>)> = entries
        .iter()
        .map(|entry| (entry.depth, entry.title.as_str(), entry.page))
        .collect();
    assert_eq!(
        rendered,
        vec![
            (0, "Chapter One", Some(0)),
            (1, "Section 1.1", Some(0)),
            (1, "Section 1.2", Some(1)),
            (2, "Subsection 1.2.1", Some(1)),
            (0, "Chapter Two", Some(2)),
        ]
    );
}

#[test]
fn flattened_outline_of_document_without_outline_is_empty() {
    let document = outline_document(Vec::new(), 1);

    assert!(document.flattened_outline().is_empty());
}

fn outline_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static PDFIUM_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    PDFIUM_TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn pdf_stream(dictionary: &[u8], data: &[u8]) -> Vec<u8> {
    let mut object = dictionary.to_vec();
    object.extend_from_slice(format!(" /Length {} >>\nstream\n", data.len()).as_bytes());
    object.extend_from_slice(data);
    object.extend_from_slice(b"\nendstream");
    object
}

fn write_pdf_objects(path: &Path, objects: &[Vec<u8>]) {
    let mut pdf = b"%PDF-1.7\n%\x80\x81\x82\x83\n".to_vec();
    let mut offsets = vec![0];
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        pdf.extend_from_slice(object);
        pdf.extend_from_slice(b"\nendobj\n");
    }

    let xref_offset = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    pdf.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets.into_iter().skip(1) {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    std::fs::write(path, pdf).unwrap();
}

fn write_outlined_pdf(path: &Path) {
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R /Outlines 7 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 600] /Contents 5 0 R >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 600] >>".to_vec(),
        pdf_stream(b"<<", b"BT /F1 10 Tf 50 550 Td (Chapter One) Tj ET"),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        b"<< /Type /Outlines /First 8 0 R /Last 11 0 R /Count 4 >>".to_vec(),
        b"<< /Title (Chapter One) /Parent 7 0 R /Next 10 0 R /First 9 0 R /Last 9 0 R /Count 1 /Dest [3 0 R /Fit] >>".to_vec(),
        b"<< /Title (Section 1.1) /Parent 8 0 R /Dest [4 0 R /Fit] >>".to_vec(),
        b"<< /Title (Chapter Two) /Parent 7 0 R /Next 11 0 R /Dest [3 0 R /Fit] >>".to_vec(),
        b"<< /Title (Website) /Parent 7 0 R /A << /S /URI /URI (https://example.com) >> >>".to_vec(),
    ];

    write_pdf_objects(path, &objects);
}

#[test]
fn extracts_outline_tree_from_pdf() {
    let _guard = outline_test_guard();
    let Ok(backend) = PdfBackend::new(None) else {
        eprintln!("skipping outline extraction test because PDFium is unavailable");
        return;
    };
    let temp = tempfile::tempdir().unwrap();
    let pdf_path = temp.path().join("outlined.pdf");
    write_outlined_pdf(&pdf_path);

    let session = backend.open_session(&pdf_path).unwrap();
    let outline = &session.document().outline;

    assert_eq!(outline.len(), 3);
    assert_eq!(outline[0].title, "Chapter One");
    assert_eq!(outline[0].page, Some(0));
    assert_eq!(outline[0].children.len(), 1);
    assert_eq!(outline[0].children[0].title, "Section 1.1");
    assert_eq!(outline[0].children[0].page, Some(1));
    assert_eq!(outline[1].title, "Chapter Two");
    assert_eq!(outline[1].page, Some(0));
    assert_eq!(outline[2].title, "Website");
    assert_eq!(outline[2].page, None);
    assert_eq!(session.document().page_count(), 2);
}

fn press(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, crossterm::event::KeyModifiers::empty()));
}

#[test]
fn outline_panel_opens_with_t_and_jumps_to_entry_page() {
    let mut app = App::new(outline_document(
        vec![OutlineNode::new("Appendix".to_string(), Some(2))],
        3,
    ));

    press(&mut app, KeyCode::Char('t'));
    assert!(app.outline_visible());
    assert_eq!(app.mode(), termpdf::app::Mode::Outline);

    press(&mut app, KeyCode::Enter);
    assert_eq!(app.cursor_page(), 2);
    assert!(
        !app.outline_visible(),
        "panel closes after a successful jump"
    );
    assert_eq!(app.mode(), termpdf::app::Mode::Normal);
    assert!(app.status().contains("outline: jumped to"));
}

#[test]
fn outline_cursor_moves_with_j_and_k_and_clamps() {
    let mut app = App::new(outline_document(
        vec![
            OutlineNode::new("One".to_string(), Some(0)),
            OutlineNode::new("Two".to_string(), Some(1)),
            OutlineNode::new("Three".to_string(), Some(2)),
        ],
        3,
    ));
    press(&mut app, KeyCode::Char('t'));

    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('j'));
    assert_eq!(app.outline_cursor(), 2);
    press(&mut app, KeyCode::Char('j'));
    assert_eq!(app.outline_cursor(), 2, "cursor clamps at the last entry");

    press(&mut app, KeyCode::Char('k'));
    assert_eq!(app.outline_cursor(), 1);
    press(&mut app, KeyCode::Char('G'));
    assert_eq!(app.outline_cursor(), 2);
    press(&mut app, KeyCode::Char('g'));
    assert_eq!(app.outline_cursor(), 0);
}

#[test]
fn outline_entry_without_page_reports_missing_destination() {
    let mut app = App::new(outline_document(
        vec![OutlineNode::new("External link".to_string(), None)],
        1,
    ));
    press(&mut app, KeyCode::Char('t'));

    press(&mut app, KeyCode::Enter);

    assert!(app.status().contains("no page destination"));
    assert_eq!(app.cursor_page(), 0);
    assert!(
        app.outline_visible(),
        "panel stays open when the entry cannot jump"
    );
}

#[test]
fn outline_toggle_without_entries_is_harmless() {
    let mut app = App::new(outline_document(Vec::new(), 1));

    press(&mut app, KeyCode::Char('t'));
    assert!(app.outline_visible());

    press(&mut app, KeyCode::Enter);
    assert!(app.status().contains("no outline entries"));

    press(&mut app, KeyCode::Esc);
    assert_eq!(app.mode(), termpdf::app::Mode::Normal);
}

#[test]
fn normal_mode_tab_keeps_focusing_images_instead_of_outline() {
    let mut app = App::new(outline_document(
        vec![OutlineNode::new("Chapter".to_string(), Some(0))],
        1,
    ));

    press(&mut app, KeyCode::Tab);
    assert!(!app.outline_visible());

    press(&mut app, KeyCode::Char('t'));
    assert!(app.outline_visible());
}
