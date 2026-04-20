mod common;

use termpdf::document::PdfRect;
use termpdf::search::DocumentIndex;

#[test]
fn builds_text_index_with_newline_between_lines() {
    let document = common::sample_document();
    let index = DocumentIndex::build(&document);

    assert_eq!(
        index.full_text,
        "alpha beta\nbeta gamma\nzeta\nbeta delta\nomega beta\n"
    );
    assert_eq!(index.chars[0].ch, 'a');
    assert_eq!(index.chars[10].ch, '\n');
}

#[test]
fn search_returns_matches_with_page_and_line() {
    let document = common::sample_document();
    let index = DocumentIndex::build(&document);

    let matches = index.search("beta");

    assert_eq!(matches.len(), 4);
    assert_eq!(matches[0].page, 0);
    assert_eq!(matches[0].line, 0);
    assert_eq!(matches[1].page, 0);
    assert_eq!(matches[1].line, 1);
    assert_eq!(matches[2].page, 1);
    assert_eq!(matches[2].line, 0);
    assert_eq!(matches[3].page, 1);
    assert_eq!(matches[3].line, 1);
}

#[test]
fn search_selection_maps_back_to_glyphs() {
    let document = common::sample_document();
    let index = DocumentIndex::build(&document);
    let search_match = index.search("gamma").remove(0);

    let glyphs = index.selection_for_match(&search_match);
    let text = glyphs.iter().map(|glyph| glyph.ch).collect::<String>();

    assert_eq!(text, "gamma");
    assert!(
        glyphs
            .iter()
            .all(|glyph| glyph.page == 0 && glyph.line == 1)
    );
}

#[test]
fn search_selection_keeps_later_page_coordinates() {
    let document = common::sample_document();
    let index = DocumentIndex::build(&document);
    let search_match = index.search("omega").remove(0);

    let glyphs = index.selection_for_match(&search_match);
    let text = glyphs.iter().map(|glyph| glyph.ch).collect::<String>();

    assert_eq!(search_match.page, 1);
    assert_eq!(search_match.line, 1);
    assert_eq!(text, "omega");
    assert!(
        glyphs
            .iter()
            .all(|glyph| glyph.page == 1 && glyph.line == 1)
    );
}

#[test]
fn selection_bounds_cover_only_matched_glyphs() {
    let document = common::sample_document();
    let index = DocumentIndex::build(&document);
    let search_match = index.search("gamma").remove(0);

    let bounds = index.selection_bounds_for_match(&search_match).unwrap();

    assert_eq!(bounds.x, 45.0);
    assert_eq!(bounds.y, 18.0);
    assert_eq!(bounds.width, 45.0);
    assert_eq!(bounds.height, 18.0);
}

#[test]
fn selection_bounds_for_page_matches_returns_all_matches_on_page() {
    let document = common::sample_document();
    let index = DocumentIndex::build(&document);
    let matches = index.search("beta");

    let page_zero = index.selection_bounds_for_page_matches(&matches, 0);
    let page_one = index.selection_bounds_for_page_matches(&matches, 1);

    assert_eq!(page_zero.len(), 2);
    assert_eq!(page_one.len(), 2);
    assert_eq!(page_zero[0], PdfRect::new(54.0, 0.0, 36.0, 18.0));
    assert_eq!(page_zero[1], PdfRect::new(0.0, 18.0, 36.0, 18.0));
}

#[test]
fn search_uses_character_indices_not_byte_offsets() {
    let document = termpdf::document::Document {
        pages: vec![termpdf::document::Page::from_text(
            0,
            &["你好 beta", "beta"],
        )],
    };
    let index = DocumentIndex::build(&document);

    let matches = index.search("beta");

    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].page, 0);
    assert_eq!(matches[0].line, 0);
    assert_eq!(matches[1].page, 0);
    assert_eq!(matches[1].line, 1);
}
