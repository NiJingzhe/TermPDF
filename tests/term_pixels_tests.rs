use termpdf::render::CellPixels;
use termpdf::term_pixels::{
    cell_pixels_from_text_area, parse_cell_size_pixels, parse_text_area_cells,
    parse_text_area_pixels,
};

#[test]
fn parses_cell_size_in_pixels_reply() {
    assert_eq!(
        parse_cell_size_pixels("\x1b[6;24;13t"),
        Some(CellPixels {
            width: 13,
            height: 24,
        })
    );
}

#[test]
fn parses_text_area_pixels_reply() {
    assert_eq!(
        parse_text_area_pixels("\x1b[4;1080;1920t"),
        Some((1920, 1080))
    );
    assert_eq!(
        parse_text_area_pixels("\x1b[5;1080;1920t"),
        Some((1920, 1080))
    );
}

#[test]
fn parses_text_area_cells_reply() {
    assert_eq!(parse_text_area_cells("\x1b[8;40;120t"), Some((120, 40)));
}

#[test]
fn ignores_replies_for_other_reports() {
    assert_eq!(parse_cell_size_pixels("\x1b[4;1080;1920t"), None);
    assert_eq!(parse_text_area_cells("\x1b[6;24;13t"), None);
}

#[test]
fn ignores_unrelated_and_malformed_input() {
    for reply in ["", "hello", "\x1b[6;24t", "\x1b[6;x;yt", "\x1b[6;;13t"] {
        assert_eq!(parse_cell_size_pixels(reply), None, "reply {reply:?}");
    }
}

#[test]
fn rejects_zero_pixel_replies() {
    assert_eq!(parse_cell_size_pixels("\x1b[6;0;0t"), None);
}

#[test]
fn finds_reply_among_other_terminal_output() {
    assert_eq!(
        parse_cell_size_pixels("\x1b[?25l\x1b[6;28;14t\x1b[0m"),
        Some(CellPixels {
            width: 14,
            height: 28,
        })
    );
}

#[test]
fn derives_cell_size_from_text_area_metrics() {
    assert_eq!(
        cell_pixels_from_text_area(1920, 1080, 120, 40),
        Some(CellPixels {
            width: 16,
            height: 27,
        })
    );
}

#[test]
fn derivation_rejects_zero_metrics() {
    assert_eq!(cell_pixels_from_text_area(1920, 1080, 0, 40), None);
    assert_eq!(cell_pixels_from_text_area(0, 0, 120, 40), None);
}
