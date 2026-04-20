use termpdf::document::{Page, PdfRect};

#[test]
fn page_from_text_keeps_page_index_on_glyphs() {
    let page = Page::from_text(2, &["abc"]);

    assert!(page.lines[0].glyphs.iter().all(|glyph| glyph.page == 2));
}

#[test]
fn page_from_text_builds_expected_line_box() {
    let page = Page::from_text(0, &["abc"]);

    assert_eq!(page.lines[0].bbox, PdfRect::new(0.0, 0.0, 27.0, 18.0));
}
