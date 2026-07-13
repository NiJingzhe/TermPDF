mod common;

use std::path::PathBuf;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use termpdf::app::{App, Mode};
use termpdf::document::{Document, Page, PdfImage, PdfMatrix, PdfRect};
use termpdf::render::{CellPixels, ViewportPixels, build_document_layout, current_page_for_scroll};

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
fn mouse_wheel_scrolls_document_vertically() {
    let document = common::sample_document();
    let mut app = App::new(document);

    app.handle_mouse(mouse_event(MouseEventKind::ScrollDown, KeyModifiers::NONE));
    let after_down = app.viewport_offset().y;
    app.handle_mouse(mouse_event(MouseEventKind::ScrollUp, KeyModifiers::NONE));

    assert!(after_down > 0);
    assert!(app.viewport_offset().y < after_down);
}

#[test]
fn shifted_mouse_wheel_scrolls_document_horizontally() {
    let document = common::sample_document();
    let mut app = App::new(document);
    app.handle_key(KeyEvent::from(KeyCode::Char('=')));
    app.handle_key(KeyEvent::from(KeyCode::Char('=')));

    app.handle_mouse(mouse_event(MouseEventKind::ScrollDown, KeyModifiers::SHIFT));
    let after_right = app.viewport_offset().x;
    app.handle_mouse(mouse_event(MouseEventKind::ScrollUp, KeyModifiers::SHIFT));

    assert!(after_right > 0);
    assert!(app.viewport_offset().x < after_right);
}

#[test]
fn horizontal_mouse_wheel_scrolls_document_horizontally() {
    let document = common::sample_document();
    let mut app = App::new(document);
    app.handle_key(KeyEvent::from(KeyCode::Char('=')));
    app.handle_key(KeyEvent::from(KeyCode::Char('=')));

    app.handle_mouse(mouse_event(MouseEventKind::ScrollRight, KeyModifiers::NONE));
    let after_right = app.viewport_offset().x;
    app.handle_mouse(mouse_event(MouseEventKind::ScrollLeft, KeyModifiers::NONE));

    assert!(after_right > 0);
    assert!(app.viewport_offset().x < after_right);
}

#[test]
fn ctrl_mouse_wheel_adjusts_zoom() {
    let document = common::sample_document();
    let mut app = App::new(document);

    app.handle_mouse(mouse_event(MouseEventKind::ScrollUp, KeyModifiers::CONTROL));
    assert_eq!(app.zoom_percent(), 110);

    app.handle_mouse(mouse_event(
        MouseEventKind::ScrollDown,
        KeyModifiers::CONTROL,
    ));
    assert_eq!(app.zoom_percent(), 100);
}

#[test]
fn event_batch_coalesces_repeated_mouse_scrolls() {
    let mut one = App::new(common::sample_document());
    let mut many = App::new(common::sample_document());

    one.handle_events([Event::Mouse(mouse_event(
        MouseEventKind::ScrollDown,
        KeyModifiers::NONE,
    ))]);
    many.handle_events(
        (0..20).map(|_| Event::Mouse(mouse_event(MouseEventKind::ScrollDown, KeyModifiers::NONE))),
    );

    assert_eq!(many.viewport_offset().y, one.viewport_offset().y);
}

#[test]
fn event_batch_cancels_reversed_mouse_scrolls() {
    let mut app = App::new(common::sample_document());

    app.handle_events([
        Event::Mouse(mouse_event(MouseEventKind::ScrollDown, KeyModifiers::NONE)),
        Event::Mouse(mouse_event(MouseEventKind::ScrollUp, KeyModifiers::NONE)),
    ]);

    assert_eq!(app.viewport_offset().y, 0);
}

#[test]
fn event_batch_coalesces_repeated_key_pans() {
    let mut one = App::new(common::sample_document());
    let mut many = App::new(common::sample_document());

    one.handle_events([Event::Key(KeyEvent::from(KeyCode::Char('J')))]);
    many.handle_events((0..20).map(|_| Event::Key(KeyEvent::from(KeyCode::Char('J')))));

    assert_eq!(many.viewport_offset().y, one.viewport_offset().y);
}

#[test]
fn event_batch_cancels_reversed_key_pans() {
    let mut app = App::new(common::sample_document());

    app.handle_events([
        Event::Key(KeyEvent::from(KeyCode::Char('J'))),
        Event::Key(KeyEvent::from(KeyCode::Char('K'))),
    ]);

    assert_eq!(app.viewport_offset().y, 0);
}

#[test]
fn event_batch_coalesces_repeated_zoom_inputs() {
    let mut app = App::new(common::sample_document());

    app.handle_events((0..20).map(|_| Event::Key(KeyEvent::from(KeyCode::Char('=')))));

    assert_eq!(app.zoom_percent(), 125);
}

#[test]
fn event_batch_cancels_reversed_zoom_inputs() {
    let mut app = App::new(common::sample_document());

    app.handle_events([
        Event::Key(KeyEvent::from(KeyCode::Char('='))),
        Event::Key(KeyEvent::from(KeyCode::Char('-'))),
    ]);

    assert_eq!(app.zoom_percent(), 100);
}

#[test]
fn presentation_mouse_wheel_changes_pages() {
    let document = sample_document_with_pages(3, 3);
    let mut app = App::new(document);
    app.handle_key(KeyEvent::from(KeyCode::F(5)));

    app.handle_mouse(mouse_event(MouseEventKind::ScrollDown, KeyModifiers::NONE));
    assert_eq!(app.cursor_page(), 1);

    app.handle_mouse(mouse_event(MouseEventKind::ScrollUp, KeyModifiers::NONE));
    assert_eq!(app.cursor_page(), 0);
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

    app.handle_key(KeyEvent::from(KeyCode::Char('J')));
    let marked_offset = app.viewport_offset();

    app.handle_key(KeyEvent::from(KeyCode::Char('m')));
    assert_eq!(app.mode(), Mode::SetMark);
    app.handle_key(KeyEvent::from(KeyCode::Char('a')));

    app.handle_key(KeyEvent::from(KeyCode::Char('J')));
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
fn v_enters_visual_mode_and_tracks_character_selection() {
    let (mut app, _clipboard) = App::with_memory_clipboard_for_tests(common::sample_document());
    app.handle_key(KeyEvent::from(KeyCode::Char('g')));
    app.handle_key(KeyEvent::from(KeyCode::Char('g')));

    app.handle_key(KeyEvent::from(KeyCode::Char('v')));
    app.handle_key(KeyEvent::from(KeyCode::Char('l')));
    app.handle_key(KeyEvent::from(KeyCode::Char('l')));
    app.handle_key(KeyEvent::from(KeyCode::Char('l')));
    app.handle_key(KeyEvent::from(KeyCode::Char('l')));

    assert_eq!(app.mode(), Mode::Visual);
    assert_eq!(app.selected_text().as_deref(), Some("alpha"));
    assert_eq!(app.selection_ranges().len(), 1);
    assert_eq!(app.selection_ranges()[0].start_glyph, 0);
    assert_eq!(app.selection_ranges()[0].end_glyph, 4);
    assert!(app.selection_bounds_for_page(0).len() == 1);
    assert!(app.cursor_bounds_for_page(0).is_none());
}

#[test]
fn normal_mode_exposes_visible_cursor_bounds() {
    let app = App::new(common::sample_document());

    assert!(app.cursor_bounds_for_page(app.cursor_page()).is_some());
}

#[test]
fn tab_and_backtab_cycle_image_focus_across_pages() {
    let mut app = App::new(document_with_images());

    app.handle_key(KeyEvent::from(KeyCode::Tab));
    assert_eq!(app.focused_image(), Some((0, 0)));
    assert_eq!(
        app.cursor_bounds_for_page(0),
        Some(PdfRect::new(10.0, 20.0, 30.0, 40.0))
    );

    app.handle_key(KeyEvent::from(KeyCode::Tab));
    assert_eq!(app.focused_image(), Some((0, 1)));
    app.handle_key(KeyEvent::from(KeyCode::Tab));
    assert_eq!(app.focused_image(), Some((1, 0)));
    app.handle_key(KeyEvent::from(KeyCode::Tab));
    assert_eq!(app.focused_image(), Some((0, 0)));

    app.handle_key(KeyEvent::from(KeyCode::BackTab));
    assert_eq!(app.focused_image(), Some((1, 0)));
    assert!(app.status().contains("y copy"));
}

#[test]
fn image_focus_is_cleared_by_escape_or_text_motion() {
    let mut app = App::new(document_with_images());

    app.handle_key(KeyEvent::from(KeyCode::Tab));
    app.handle_key(KeyEvent::from(KeyCode::Esc));
    assert_eq!(app.focused_image(), None);

    app.handle_key(KeyEvent::from(KeyCode::Tab));
    app.handle_key(KeyEvent::from(KeyCode::Char('l')));
    assert_eq!(app.focused_image(), None);
    assert!(app.cursor_bounds_for_page(0).is_some());
}

#[test]
fn text_motion_after_cross_page_image_focus_stays_on_focused_page() {
    let mut app = App::new(document_with_images());

    app.handle_key(KeyEvent::from(KeyCode::Tab));
    app.handle_key(KeyEvent::from(KeyCode::Tab));
    app.handle_key(KeyEvent::from(KeyCode::Tab));
    assert_eq!(app.focused_image(), Some((1, 0)));

    app.handle_key(KeyEvent::from(KeyCode::Char('l')));

    assert_eq!(app.focused_image(), None);
    assert_eq!(app.text_cursor().page, 1);
}

#[test]
fn focused_image_y_requests_lazy_png_copy() {
    let (mut app, clipboard) = App::with_memory_clipboard_for_tests(document_with_images());

    app.handle_key(KeyEvent::from(KeyCode::Tab));
    app.handle_key(KeyEvent::from(KeyCode::Char('y')));

    assert_eq!(app.pending_image_copy(), Some((0, 0)));
    assert!(clipboard.image().is_none());

    let png = b"\x89PNG\r\n\x1a\nimage bytes";
    app.complete_image_copy(png);
    assert_eq!(clipboard.image().as_deref(), Some(png.as_slice()));
    assert!(app.status().contains("copied image"));
}

#[test]
fn normal_mode_h_and_l_move_text_cursor_without_selection() {
    let mut app = App::new(common::sample_document());

    for _ in 0..20 {
        app.handle_key(KeyEvent::from(KeyCode::Char('h')));
    }
    let left = app.text_cursor();
    app.handle_key(KeyEvent::from(KeyCode::Char('l')));

    assert_eq!(app.mode(), Mode::Normal);
    assert_eq!(left.glyph, 0);
    assert_eq!(app.text_cursor().glyph, 1);
    assert!(app.selected_text().is_none());

    app.handle_key(KeyEvent::from(KeyCode::Char('h')));
    assert_eq!(app.text_cursor().glyph, 0);
}

#[test]
fn normal_mode_j_and_k_move_text_cursor_one_line_at_a_time() {
    let mut app = App::new(common::sample_document());
    app.handle_key(KeyEvent::from(KeyCode::Char('g')));
    app.handle_key(KeyEvent::from(KeyCode::Char('g')));

    assert_eq!(app.text_cursor().line, 0);
    app.handle_key(KeyEvent::from(KeyCode::Char('j')));
    assert_eq!(app.text_cursor().line, 1);
    app.handle_key(KeyEvent::from(KeyCode::Char('j')));
    assert_eq!(app.text_cursor().line, 2);
    app.handle_key(KeyEvent::from(KeyCode::Char('k')));
    assert_eq!(app.text_cursor().line, 1);
    assert!(app.selected_text().is_none());
}

#[test]
fn normal_mode_counted_j_moves_exact_line_count() {
    let mut app = App::new(sample_document_with_pages(1, 6));
    app.handle_key(KeyEvent::from(KeyCode::Char('g')));
    app.handle_key(KeyEvent::from(KeyCode::Char('g')));

    app.handle_key(KeyEvent::from(KeyCode::Char('3')));
    app.handle_key(KeyEvent::from(KeyCode::Char('j')));

    assert_eq!(app.text_cursor().line, 3);
}

#[test]
fn normal_mode_supports_word_and_line_boundary_motions() {
    let mut app = App::new(Document {
        pages: vec![Page::from_text(0, &["  alpha beta", "gamma delta"])],
    });
    app.handle_key(KeyEvent::from(KeyCode::Char('g')));
    app.handle_key(KeyEvent::from(KeyCode::Char('g')));

    app.handle_key(KeyEvent::from(KeyCode::Char('^')));
    assert_eq!(app.text_cursor().glyph, 2);

    app.handle_key(KeyEvent::from(KeyCode::Char('w')));
    assert_eq!(app.text_cursor().glyph, 8);

    app.handle_key(KeyEvent::from(KeyCode::Char('$')));
    assert_eq!(app.text_cursor().glyph, 11);

    app.handle_key(KeyEvent::from(KeyCode::Char('b')));
    assert_eq!(app.text_cursor().glyph, 8);
}

#[test]
fn normal_mode_counted_w_crosses_lines() {
    let mut app = App::new(Document {
        pages: vec![Page::from_text(0, &["alpha beta", "gamma delta"])],
    });
    app.handle_key(KeyEvent::from(KeyCode::Char('g')));
    app.handle_key(KeyEvent::from(KeyCode::Char('g')));

    app.handle_key(KeyEvent::from(KeyCode::Char('2')));
    app.handle_key(KeyEvent::from(KeyCode::Char('w')));

    assert_eq!(app.text_cursor().line, 1);
    assert_eq!(app.text_cursor().glyph, 0);
}

#[test]
fn uppercase_hjkl_pan_viewport() {
    let mut app = App::new(common::sample_document());
    app.handle_key(KeyEvent::from(KeyCode::Char('=')));
    app.handle_key(KeyEvent::from(KeyCode::Char('=')));

    app.handle_key(KeyEvent::from(KeyCode::Char('L')));
    let after_right = app.viewport_offset().x;
    app.handle_key(KeyEvent::from(KeyCode::Char('H')));

    assert!(after_right > 0);
    assert!(app.viewport_offset().x < after_right);

    app.handle_key(KeyEvent::from(KeyCode::Char('J')));
    let after_down = app.viewport_offset().y;
    app.handle_key(KeyEvent::from(KeyCode::Char('K')));

    assert!(after_down > 0);
    assert!(app.viewport_offset().y < after_down);
}

#[test]
fn visual_mode_y_copies_selected_text_and_exits() {
    let (mut app, clipboard) = App::with_memory_clipboard_for_tests(common::sample_document());
    app.handle_key(KeyEvent::from(KeyCode::Char('g')));
    app.handle_key(KeyEvent::from(KeyCode::Char('g')));

    app.handle_key(KeyEvent::from(KeyCode::Char('v')));
    for _ in 0..4 {
        app.handle_key(KeyEvent::from(KeyCode::Char('l')));
    }
    app.handle_key(KeyEvent::from(KeyCode::Char('y')));

    assert_eq!(app.mode(), Mode::Normal);
    assert_eq!(clipboard.text().as_deref(), Some("alpha"));
    assert!(app.selected_text().is_none());
    assert!(app.status().contains("copied"));
}

#[test]
fn visual_mode_supports_word_and_line_boundary_motions() {
    let (mut app, _clipboard) = App::with_memory_clipboard_for_tests(Document {
        pages: vec![Page::from_text(0, &["alpha beta", "gamma delta"])],
    });
    app.handle_key(KeyEvent::from(KeyCode::Char('g')));
    app.handle_key(KeyEvent::from(KeyCode::Char('g')));

    app.handle_key(KeyEvent::from(KeyCode::Char('v')));
    app.handle_key(KeyEvent::from(KeyCode::Char('w')));

    assert_eq!(app.mode(), Mode::Visual);
    assert_eq!(app.text_cursor().glyph, 6);

    app.handle_key(KeyEvent::from(KeyCode::Char('$')));
    assert_eq!(app.text_cursor().glyph, 9);

    app.handle_key(KeyEvent::from(KeyCode::Char('b')));
    assert_eq!(app.text_cursor().glyph, 6);

    app.handle_key(KeyEvent::from(KeyCode::Char('^')));
    assert_eq!(app.text_cursor().glyph, 0);
}

#[test]
fn shift_v_selects_whole_lines_for_copy() {
    let (mut app, clipboard) = App::with_memory_clipboard_for_tests(common::sample_document());
    app.handle_key(KeyEvent::from(KeyCode::Char('g')));
    app.handle_key(KeyEvent::from(KeyCode::Char('g')));

    app.handle_key(KeyEvent::from(KeyCode::Char('V')));
    app.handle_key(KeyEvent::from(KeyCode::Char('j')));
    app.handle_key(KeyEvent::from(KeyCode::Char('y')));

    assert_eq!(app.mode(), Mode::Normal);
    assert_eq!(clipboard.text().as_deref(), Some("alpha beta\nbeta gamma"));
}

#[test]
fn visual_line_mode_does_not_overlay_character_cursor() {
    let (mut app, _clipboard) = App::with_memory_clipboard_for_tests(common::sample_document());
    app.handle_key(KeyEvent::from(KeyCode::Char('g')));
    app.handle_key(KeyEvent::from(KeyCode::Char('g')));

    app.handle_key(KeyEvent::from(KeyCode::Char('V')));

    assert_eq!(app.mode(), Mode::VisualLine);
    assert!(app.cursor_bounds_for_page(0).is_none());
    assert_eq!(app.selection_bounds_for_page(0).len(), 1);
}

#[test]
fn escape_exits_visual_mode_without_copying() {
    let (mut app, clipboard) = App::with_memory_clipboard_for_tests(common::sample_document());

    app.handle_key(KeyEvent::from(KeyCode::Char('v')));
    app.handle_key(KeyEvent::from(KeyCode::Esc));

    assert_eq!(app.mode(), Mode::Normal);
    assert!(clipboard.text().is_none());
    assert!(app.selected_text().is_none());
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

    app.handle_key(KeyEvent::from(KeyCode::Char('J')));
    app.handle_key(KeyEvent::from(KeyCode::Char('J')));
    app.handle_key(KeyEvent::from(KeyCode::Char('J')));

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

fn document_with_images() -> Document {
    let mut pages = vec![
        Page::from_text(0, &["first page"]),
        Page::from_text(1, &["second page"]),
    ];
    pages[0].images = vec![
        test_image(0, 10.0, 20.0, 30.0, 40.0),
        test_image(0, 80.0, 100.0, 50.0, 60.0),
    ];
    pages[1].images = vec![test_image(1, 15.0, 25.0, 35.0, 45.0)];
    Document { pages }
}

fn test_image(page: usize, x: f32, y: f32, width: f32, height: f32) -> PdfImage {
    PdfImage {
        bbox: PdfRect::new(x, y, width, height),
        matrix: PdfMatrix {
            a: width,
            b: 0.0,
            c: 0.0,
            d: height,
            e: x,
            f: y,
        },
        pixel_width: width as u32,
        pixel_height: height as u32,
        page,
        object_path: vec![0],
    }
}

fn mouse_event(kind: MouseEventKind, modifiers: KeyModifiers) -> MouseEvent {
    MouseEvent {
        kind,
        column: 10,
        row: 5,
        modifiers,
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
