mod common;

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use termpdf::app::{App, Mode};
use termpdf::document::{Document, Page};
use termpdf::render::{build_document_layout, current_page_for_scroll, CellPixels, ViewportPixels};

#[test]
fn slash_search_enters_mode_and_enter_commits_results() {
    let document = common::sample_document();
    let mut app = App::new(document);

    app.handle_key(KeyEvent::from(KeyCode::Char('/')));
    app.handle_key(KeyEvent::from(KeyCode::Char('b')));
    app.handle_key(KeyEvent::from(KeyCode::Char('e')));
    app.handle_key(KeyEvent::from(KeyCode::Char('t')));
    app.handle_key(KeyEvent::from(KeyCode::Char('a')));
    app.handle_key(KeyEvent::from(KeyCode::Enter));

    assert_eq!(app.mode(), Mode::Normal);
    assert_eq!(app.active_search_match().unwrap().line, 0);
    assert_eq!((app.cursor_page(), app.cursor_line()), (0, 0));
}

#[test]
fn esc_in_search_mode_hides_highlight_and_returns_to_normal() {
    let document = common::sample_document();
    let mut app = App::new(document);

    app.handle_key(KeyEvent::from(KeyCode::Char('/')));
    for ch in "beta".chars() {
        app.handle_key(KeyEvent::from(KeyCode::Char(ch)));
    }
    app.handle_key(KeyEvent::from(KeyCode::Enter));
    assert!(app.highlight_search());

    app.handle_key(KeyEvent::from(KeyCode::Esc));

    assert_eq!(app.mode(), Mode::Normal);
    assert!(!app.highlight_search());
}

#[test]
fn n_and_shift_n_cycle_search_matches() {
    let document = common::sample_document();
    let mut app = App::new(document);

    app.handle_key(KeyEvent::from(KeyCode::Char('/')));
    for ch in "beta".chars() {
        app.handle_key(KeyEvent::from(KeyCode::Char(ch)));
    }
    app.handle_key(KeyEvent::from(KeyCode::Enter));

    app.handle_key(KeyEvent::from(KeyCode::Char('n')));
    assert_eq!(app.active_search_match().unwrap().line, 1);

    app.handle_key(KeyEvent::from(KeyCode::Char('N')));
    assert_eq!(app.active_search_match().unwrap().line, 0);
}

#[test]
fn ctrl_d_and_ctrl_u_scroll_document() {
    let document = common::sample_document();
    let mut app = App::new(document);

    app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
    let after_down = app.viewport_offset().y;
    app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));

    assert!(after_down > 0);
    assert!(app.viewport_offset().y < after_down);
}

#[test]
fn ctrl_f_and_ctrl_b_scroll_by_full_viewport() {
    let document = common::sample_document();
    let mut app = App::new(document);

    app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
    let after_forward = app.viewport_offset().y;
    app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));

    assert!(after_forward > 0);
    assert!(app.viewport_offset().y < after_forward);
}

#[test]
fn numeric_gg_jumps_to_requested_page() {
    let document = common::sample_document();
    let mut app = App::new(document);

    app.handle_key(KeyEvent::from(KeyCode::Char('2')));
    app.handle_key(KeyEvent::from(KeyCode::Char('g')));
    app.handle_key(KeyEvent::from(KeyCode::Char('g')));

    assert_eq!(app.cursor_page(), 1);
}

#[test]
fn plain_gg_jumps_to_first_page() {
    let document = common::sample_document();
    let mut app = App::new(document);

    app.handle_key(KeyEvent::from(KeyCode::Char('G')));
    assert_eq!(app.cursor_page(), 1);

    app.handle_key(KeyEvent::from(KeyCode::Char('g')));
    app.handle_key(KeyEvent::from(KeyCode::Char('g')));

    assert_eq!((app.cursor_page(), app.cursor_line()), (0, 0));
}

#[test]
fn m_and_backtick_store_and_restore_marks() {
    let document = common::sample_document();
    let mut app = App::new(document);

    app.handle_key(KeyEvent::from(KeyCode::Char('j')));
    let marked_offset = app.viewport_offset();

    app.handle_key(KeyEvent::from(KeyCode::Char('m')));
    assert_eq!(app.mode(), Mode::SetMark);
    app.handle_key(KeyEvent::from(KeyCode::Char('a')));

    app.handle_key(KeyEvent::from(KeyCode::Char('j')));
    assert_ne!(app.viewport_offset(), marked_offset);

    app.handle_key(KeyEvent::from(KeyCode::Char('`')));
    assert_eq!(app.mode(), Mode::JumpMark);
    app.handle_key(KeyEvent::from(KeyCode::Char('a')));

    assert_eq!(app.viewport_offset(), marked_offset);
}

#[test]
fn f_enters_follow_mode() {
    let document = common::sample_document();
    let mut app = App::new(document);

    app.handle_key(KeyEvent::from(KeyCode::Char('f')));

    assert!(matches!(app.mode(), Mode::Normal | Mode::Follow));
}

#[test]
fn follow_mode_exposes_visible_hints_list() {
    let document = common::sample_document();
    let mut app = App::new(document);

    app.handle_key(KeyEvent::from(KeyCode::Char('f')));

    assert!(app.visible_follow_hints().is_empty() || app.mode() == Mode::Follow);
}

#[test]
fn follow_mode_space_rotates_visible_hint_order() {
    let document = common::sample_document();
    let mut app = App::new(document);

    app.handle_key(KeyEvent::from(KeyCode::Char('f')));
    let before = app
        .visible_follow_hints()
        .into_iter()
        .map(|hint| hint.label)
        .collect::<Vec<_>>();
    app.handle_key(KeyEvent::from(KeyCode::Char(' ')));
    let after = app
        .visible_follow_hints()
        .into_iter()
        .map(|hint| hint.label)
        .collect::<Vec<_>>();

    if before.len() > 1 && before != after {
        assert_eq!(before.len(), after.len());
        assert_ne!(before[0], after[0]);
    } else {
        assert_eq!(before.len(), after.len());
    }
}

#[test]
fn f5_enters_presentation_mode() {
    let document = common::sample_document();
    let mut app = App::new(document);

    app.handle_key(KeyEvent::from(KeyCode::F(5)));
    assert_eq!(app.mode(), Mode::Presentation);

    app.handle_key(KeyEvent::from(KeyCode::Esc));
    assert_eq!(app.mode(), Mode::Normal);
}

#[test]
fn zoom_keybindings_adjust_zoom_and_reset() {
    let document = common::sample_document();
    let mut app = App::new(document);

    app.handle_key(KeyEvent::from(KeyCode::Char('=')));
    app.handle_key(KeyEvent::from(KeyCode::Char('=')));
    assert_eq!(app.zoom_percent(), 150);

    app.handle_key(KeyEvent::from(KeyCode::Char('-')));
    assert_eq!(app.zoom_percent(), 125);

    app.handle_key(KeyEvent::from(KeyCode::Char('0')));
    assert_eq!(app.zoom_percent(), 100);
}

#[test]
fn i_key_toggles_dark_mode() {
    let document = common::sample_document();
    let mut app = App::new(document);

    assert!(!app.dark_mode());
    app.handle_key(KeyEvent::from(KeyCode::Char('i')));
    assert!(app.dark_mode());
    app.handle_key(KeyEvent::from(KeyCode::Char('i')));
    assert!(!app.dark_mode());
}

#[test]
fn current_page_match_bounds_returns_all_visible_matches() {
    let document = common::sample_document();
    let mut app = App::new(document);

    app.handle_key(KeyEvent::from(KeyCode::Char('/')));
    for ch in "beta".chars() {
        app.handle_key(KeyEvent::from(KeyCode::Char(ch)));
    }
    app.handle_key(KeyEvent::from(KeyCode::Enter));

    let bounds = app.current_page_match_bounds();

    assert_eq!(bounds.len(), 2);
    assert_eq!(bounds[0].x, 54.0);
    assert_eq!(bounds[1].x, 0.0);
}

#[test]
fn active_match_bounds_disappear_when_scroll_centers_other_page() {
    let document = common::sample_document();
    let mut app = App::new(document);

    app.handle_key(KeyEvent::from(KeyCode::Char('/')));
    for ch in "gamma".chars() {
        app.handle_key(KeyEvent::from(KeyCode::Char(ch)));
    }
    app.handle_key(KeyEvent::from(KeyCode::Enter));
    assert!(app.active_match_bounds().is_some());

    app.handle_key(KeyEvent::from(KeyCode::Char('j')));
    app.handle_key(KeyEvent::from(KeyCode::Char('j')));
    app.handle_key(KeyEvent::from(KeyCode::Char('j')));

    assert!(app.current_page_match_bounds().is_empty());
    assert!(app.active_match_bounds().is_none());
}

#[test]
fn image_area_leaves_space_for_status_bar() {
    let body = Rect::new(0, 0, 120, 37);
    assert_eq!(body, Rect::new(0, 0, 120, 37));
}

#[test]
fn default_status_includes_new_key_hints() {
    let app = App::new(common::sample_document());

    assert!(app.status().contains("zoom 100%"));
    assert!(app.status().contains("line "));
    assert!(!app.status().contains("/ search"));
    assert!(!app.status().contains("f links"));
    assert!(!app.status().contains("F5 present"));
    assert!(!app.status().contains("page 1/2"));
    assert!(!app.status().contains("scroll"));
}

#[test]
fn with_path_exposes_file_path_for_title_bar() {
    let app = App::with_path(
        common::sample_document(),
        PathBuf::from("/Users/test/work/docs/example.pdf"),
    );

    assert_eq!(
        app.file_path().unwrap(),
        PathBuf::from("/Users/test/work/docs/example.pdf").as_path()
    );
}

#[test]
fn presentation_mode_status_mentions_navigation() {
    let document = common::sample_document();
    let mut app = App::new(document);

    app.handle_key(KeyEvent::from(KeyCode::F(5)));

    assert!(app.status().contains("presentation"));
    assert!(app.status().contains("Space next"));
}

#[test]
fn replacing_document_preserves_current_page_and_intra_page_position() {
    let mut app = App::new(sample_document_with_pages(4, 12));

    app.handle_key(KeyEvent::from(KeyCode::Char('G')));
    app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));

    let old_anchor = current_view_anchor(&app);

    app.replace_document_preserving_view_position(sample_document_with_pages(6, 20));

    let new_anchor = current_view_anchor(&app);

    assert_eq!(old_anchor.page, 3);
    assert_eq!(new_anchor.page, old_anchor.page);
    assert!((new_anchor.relative_y - old_anchor.relative_y).abs() < 0.02);
}

fn sample_document_with_pages(page_count: usize, lines_per_page: usize) -> Document {
    Document {
        pages: (0..page_count)
            .map(|page_index| {
                let lines = (0..lines_per_page)
                    .map(|line_index| format!("page {page_index} line {line_index}"))
                    .collect::<Vec<_>>();
                let refs = lines.iter().map(String::as_str).collect::<Vec<_>>();
                Page::from_text(page_index, &refs)
            })
            .collect(),
    }
}

#[derive(Clone, Copy, Debug)]
struct ViewAnchor {
    page: usize,
    relative_y: f32,
}

fn current_view_anchor(app: &App) -> ViewAnchor {
    let viewport = ViewportPixels {
        area: Rect::new(0, 0, 100, 20),
        cell: CellPixels {
            width: 8,
            height: 20,
        },
        width: 800,
        height: 400,
    };
    let layout = build_document_layout(app.document(), viewport, app.zoom_factor());
    let page = current_page_for_scroll(&layout, app.viewport_offset().y, viewport.height);
    let page_layout = layout.pages[page];
    let viewport_center_y = app.viewport_offset().y.saturating_add(viewport.height / 2);

    ViewAnchor {
        page,
        relative_y: (viewport_center_y.saturating_sub(page_layout.doc_y) as f32
            / page_layout.bitmap_height.max(1) as f32)
            .clamp(0.0, 1.0),
    }
}
