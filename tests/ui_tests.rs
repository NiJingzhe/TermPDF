use ratatui::layout::Rect;
use termpdf::document::PdfRect;
use termpdf::render::CellPixels;
use termpdf::ui::{
    display_path, highlight_overlay_placement, inner_image_area, project_pdf_rect_to_cells,
    viewport_area, visible_scroll,
};

#[test]
fn visible_scroll_keeps_cursor_in_view() {
    assert_eq!(visible_scroll(0, 4), 0);
    assert_eq!(visible_scroll(3, 4), 0);
    assert_eq!(visible_scroll(4, 4), 1);
    assert_eq!(visible_scroll(9, 4), 6);
}

#[test]
fn projects_pdf_rect_into_terminal_cells() {
    let page_bbox = PdfRect::new(0.0, 0.0, 200.0, 100.0);
    let glyph_bbox = PdfRect::new(50.0, 25.0, 40.0, 10.0);
    let cell_area = Rect::new(10, 5, 80, 20);

    let projected = project_pdf_rect_to_cells(page_bbox, glyph_bbox, cell_area);

    assert_eq!(projected, Rect::new(30, 10, 16, 2));
}

#[test]
fn highlight_overlay_placement_preserves_pixel_offset_inside_cells() {
    let placement = highlight_overlay_placement(
        PdfRect::new(0.0, 0.0, 200.0, 100.0),
        PdfRect::new(51.0, 26.0, 40.0, 10.0),
        Rect::new(10, 5, 80, 20),
        CellPixels {
            width: 8,
            height: 10,
        },
    )
    .unwrap();

    assert_eq!(placement.cell_x, 12);
    assert_eq!(placement.cell_y, 5);
    assert_eq!(placement.offset_x, 4);
    assert_eq!(placement.offset_y, 5);
    assert_eq!(placement.columns, 3);
    assert_eq!(placement.rows, 1);
}

#[test]
fn inner_image_area_uses_block_inner_rect() {
    let area = Rect::new(2, 4, 20, 10);

    assert_eq!(inner_image_area(area), Rect::new(3, 5, 18, 8));
}

#[test]
fn display_path_condenses_long_path() {
    let path = std::path::Path::new("/Users/test/workspace/project/docs/example.pdf");

    assert_eq!(
        display_path(Some(path)),
        "/Users/.../workspace/project/docs/example.pdf"
    );
}

#[test]
fn display_path_keeps_paths_with_five_segments() {
    let path = std::path::Path::new("/Users/test/docs/example.pdf");

    assert_eq!(display_path(Some(path)), "/Users/test/docs/example.pdf");
}

#[test]
fn presentation_viewport_uses_full_frame() {
    let area = Rect::new(0, 0, 120, 40);

    assert_eq!(viewport_area(area, true), area);
}

#[test]
fn normal_viewport_reserves_status_bar() {
    let area = Rect::new(0, 0, 120, 40);

    assert_eq!(viewport_area(area, false), Rect::new(1, 1, 118, 35));
}
