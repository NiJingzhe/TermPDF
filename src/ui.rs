use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line as TextLine, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::{App, Mode};
use crate::document::PdfRect;
use crate::render::{CellPixels, OverlayPlacement};

pub(crate) fn render(frame: &mut Frame, app: &App) {
    if app.mode() == Mode::Presentation {
        let area = frame.area();
        frame.render_widget(document_paragraph(app, area), area);
    } else {
        let [body, status] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(3)]).areas(frame.area());

        frame.render_widget(document_paragraph(app, body), body);
        frame.render_widget(status_paragraph(app), status);
    }
}

pub fn viewport_area(area: Rect, presentation: bool) -> Rect {
    if presentation {
        area
    } else {
        let [body, _status] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(3)]).areas(area);
        inner_image_area(body)
    }
}

pub fn inner_image_area(area: Rect) -> Rect {
    Block::default().borders(Borders::ALL).inner(area)
}

pub fn display_path(path: Option<&std::path::Path>) -> String {
    let Some(path) = path else {
        return "<unknown file>".to_string();
    };

    let display = path.display().to_string();
    let leading_slash = display.starts_with('/');
    let components = display
        .split('/')
        .filter(|component| !component.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();

    if components.len() <= 5 {
        return display;
    }

    let tail = &components[components.len() - 4..];

    let condensed = format!("{}/.../{}", components[0], tail.join("/"));

    if leading_slash {
        format!("/{condensed}")
    } else {
        condensed
    }
}

fn document_paragraph(app: &App, area: Rect) -> Paragraph<'static> {
    let page = &app.document().pages[app.cursor_page()];
    let title = format!("{} | {}", page_label(app), display_path(app.file_path()));

    if app.kitty_supported() {
        if app.mode() == Mode::Presentation {
            return Paragraph::new(String::new());
        }

        return Paragraph::new(String::new())
            .block(Block::default().title(title).borders(Borders::ALL));
    }

    let active_line = app.cursor_line();
    let active_match = app.active_search_match();

    let lines = page
        .lines
        .iter()
        .enumerate()
        .map(|(line_index, line)| {
            let is_cursor = line_index == active_line;
            let is_match = active_match
                .map(|matched| matched.page == app.cursor_page() && matched.line == line_index)
                .unwrap_or(false);

            let mut style = Style::default();
            if is_match {
                style = style.fg(Color::Black).bg(Color::Yellow);
            }
            if is_cursor {
                style = style.add_modifier(Modifier::BOLD).fg(Color::Cyan);
            }

            TextLine::from(Span::styled(line.text(), style))
        })
        .collect::<Vec<_>>();

    if app.mode() == Mode::Presentation {
        Paragraph::new(lines).scroll((visible_scroll(active_line, area.height), 0))
    } else {
        Paragraph::new(lines)
            .block(Block::default().title(title).borders(Borders::ALL))
            .scroll((
                visible_scroll(active_line, area.height.saturating_sub(2)),
                0,
            ))
    }
}

fn status_paragraph(app: &App) -> Paragraph<'static> {
    let chips = vec![
        mode_prefix(app.mode()),
        status_chip("/", "search", Color::Yellow, Color::Black),
        separator(),
        status_chip("f", "links", Color::Cyan, Color::Black),
        separator(),
        status_chip("m", "mark", Color::Magenta, Color::Black),
        separator(),
        status_chip("F5", "present", Color::Blue, Color::White),
        separator(),
        status_chip("q", "quit", Color::Red, Color::White),
        Span::raw("  "),
        Span::styled(app.status().to_string(), Style::default().fg(Color::Gray)),
    ];

    Paragraph::new(TextLine::from(chips)).block(
        Block::default()
            .borders(Borders::ALL)
            .style(Style::default().bg(Color::Rgb(20, 24, 32))),
    )
}

fn page_label(app: &App) -> String {
    format!(
        "Page {}/{}",
        app.cursor_page() + 1,
        app.document().page_count()
    )
}

fn mode_prefix(mode: Mode) -> Span<'static> {
    match mode {
        Mode::Normal => Span::raw(String::new()),
        Mode::Search => status_chip("SEARCH", "", Color::Yellow, Color::Black),
        Mode::Follow => status_chip("FOLLOW", "", Color::Cyan, Color::Black),
        Mode::SetMark => status_chip("MARK", "", Color::Magenta, Color::Black),
        Mode::JumpMark => status_chip("JUMP", "", Color::Magenta, Color::Black),
        Mode::Presentation => status_chip("PRESENT", "", Color::Blue, Color::White),
    }
}

fn separator() -> Span<'static> {
    Span::styled(" ", Style::default().bg(Color::Rgb(20, 24, 32)))
}

fn status_chip(key: &str, label: &str, bg: Color, fg: Color) -> Span<'static> {
    let text = if label.is_empty() {
        format!(" {key} ")
    } else {
        format!(" {key} {label} ")
    };
    Span::styled(
        text,
        Style::default().bg(bg).fg(fg).add_modifier(Modifier::BOLD),
    )
}

pub fn visible_scroll(active_line: usize, viewport_height: u16) -> u16 {
    let viewport_height = viewport_height as usize;
    if viewport_height == 0 || active_line < viewport_height {
        0
    } else {
        (active_line - viewport_height + 1) as u16
    }
}

pub fn highlight_overlay_placement(
    page_bbox: PdfRect,
    match_bounds: PdfRect,
    image_area: Rect,
    cell: CellPixels,
) -> Option<OverlayPlacement> {
    if page_bbox.width <= 0.0
        || page_bbox.height <= 0.0
        || image_area.width == 0
        || image_area.height == 0
        || cell.width == 0
        || cell.height == 0
    {
        return None;
    }

    let scale_x = image_area.width as f32 / page_bbox.width;
    let scale_y = image_area.height as f32 / page_bbox.height;
    let px_left = (match_bounds.x * scale_x).floor().max(0.0) as u16;
    let px_top = (match_bounds.y * scale_y).floor().max(0.0) as u16;
    let px_width = ((match_bounds.width * scale_x).ceil() as u16).max(1);
    let px_height = ((match_bounds.height * scale_y).ceil() as u16).max(1);

    let cell_x = image_area.x + px_left / cell.width;
    let cell_y = image_area.y + px_top / cell.height;
    let offset_x = px_left % cell.width;
    let offset_y = px_top % cell.height;
    let columns =
        (u32::from(offset_x) + u32::from(px_width)).div_ceil(u32::from(cell.width)) as u16;
    let rows = (u32::from(offset_y) + u32::from(px_height)).div_ceil(u32::from(cell.height)) as u16;

    Some(OverlayPlacement {
        cell_x,
        cell_y,
        columns: columns.max(1),
        rows: rows.max(1),
        offset_x,
        offset_y,
        width_px: px_width,
        height_px: px_height,
        cell,
    })
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn project_pdf_rect_to_cells(
    page_bbox: PdfRect,
    glyph_bbox: PdfRect,
    image_area: Rect,
) -> Rect {
    if page_bbox.width <= 0.0
        || page_bbox.height <= 0.0
        || image_area.width == 0
        || image_area.height == 0
    {
        return Rect::new(image_area.x, image_area.y, 0, 0);
    }

    let scale_x = image_area.width as f32 / page_bbox.width;
    let scale_y = image_area.height as f32 / page_bbox.height;

    let left = image_area.x + (glyph_bbox.x * scale_x).floor() as u16;
    let top = image_area.y + (glyph_bbox.y * scale_y).floor() as u16;
    let width = ((glyph_bbox.width * scale_x).ceil() as u16).max(1);
    let height = ((glyph_bbox.height * scale_y).ceil() as u16).max(1);

    let max_right = image_area.x + image_area.width;
    let max_bottom = image_area.y + image_area.height;
    let clamped_left = left.min(max_right);
    let clamped_top = top.min(max_bottom);
    let clamped_width = width.min(max_right.saturating_sub(clamped_left));
    let clamped_height = height.min(max_bottom.saturating_sub(clamped_top));

    Rect::new(clamped_left, clamped_top, clamped_width, clamped_height)
}
