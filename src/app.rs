use std::collections::HashMap;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal;
use ratatui::layout::Rect;
use ratatui::DefaultTerminal;

use crate::document::{Document, LinkTarget, PageLink, PdfRect};
use crate::kitty::{wrap_command_for_transport, KittyTransport, RendererState};
use crate::pdf::{PdfBackend, PdfSession};
use crate::platform::{likely_supports_kitty_graphics, running_inside_tmux};
use crate::render::{
    build_document_layout, build_page_render_plan, build_visible_page_plans,
    compose_visible_page_frame, compose_visible_page_frame_with_offsets, current_page_for_scroll,
    follow_tag_badge_size, viewport_pixels, CellPixels, DocumentLayout, DocumentLayoutPage,
    FollowTag, FrameOffsets, PageRenderPlan, ViewportOffset, ViewportPixels,
};
use crate::search::{DocumentIndex, SearchMatch};
use crate::ui::{render, viewport_area};

const VIEWPORT_PAN_STEP_PX: u32 = 120;
const DEFAULT_VIEWPORT_AREA: Rect = Rect::new(0, 0, 100, 20);
const DEFAULT_CELL: CellPixels = CellPixels {
    width: 8,
    height: 20,
};
const WATCH_POLL_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunOptions {
    pub watch_mode: bool,
}

impl RunOptions {
    pub const fn new(watch_mode: bool) -> Self {
        Self { watch_mode }
    }
}

#[derive(Clone, Debug)]
struct FileWatchState {
    path: PathBuf,
    last_modified: Option<SystemTime>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Search,
    Follow,
    SetMark,
    JumpMark,
    Presentation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ViewMark {
    viewport_offset: ViewportOffset,
    zoom_percent: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FollowHint {
    label: String,
    page: usize,
    link_index: usize,
    area: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct FollowClusterKey {
    page: usize,
    first_link_index: usize,
}

#[derive(Clone, Debug)]
pub struct VisibleFollowHint {
    pub label: String,
    pub area: Rect,
}

#[derive(Clone, Debug)]
pub struct App {
    document: Document,
    index: DocumentIndex,
    pdf_path: Option<PathBuf>,
    zoom_percent: u16,
    viewport_offset: ViewportOffset,
    viewport: Option<ViewportPixels>,
    dark_mode: bool,
    search_input: String,
    matches: Vec<SearchMatch>,
    active_match: Option<usize>,
    highlight_search: bool,
    mode: Mode,
    status: String,
    kitty_supported: bool,
    render_nonce: u64,
    should_quit: bool,
    count_buffer: String,
    pending_g: bool,
    follow_input: String,
    follow_hints: Vec<FollowHint>,
    follow_cluster_rotations: HashMap<FollowClusterKey, usize>,
    marks: HashMap<char, ViewMark>,
    presentation_page: Option<usize>,
}

pub fn run(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    backend: &PdfBackend,
    session: &mut PdfSession,
    options: RunOptions,
) -> io::Result<()> {
    let mut renderer_state = RendererState::default();
    let mut needs_redraw = true;
    let mut watch_state = if options.watch_mode {
        Some(FileWatchState::from_path(session.pdf_path()))
    } else {
        None
    };
    let transport = if running_inside_tmux() {
        KittyTransport::TmuxPassthrough
    } else {
        KittyTransport::Direct
    };

    execute!(io::stdout(), EnableMouseCapture)?;

    app.kitty_supported = likely_supports_kitty_graphics();
    app.status = app.default_status();

    while !app.should_quit {
        if needs_redraw {
            let completed = terminal.draw(|frame| render(frame, app))?;

            let overlay_area = viewport_area(completed.area, app.mode() == Mode::Presentation);
            if app.kitty_supported() {
                let image_area = overlay_area;
                maybe_render_page_images(app, session, image_area, &mut renderer_state, transport)?;
            }

            needs_redraw = false;
        }

        if let Some(watch_state) = watch_state.as_mut() {
            if let Some(document) = maybe_reload_document(backend, session, watch_state)? {
                app.replace_document_preserving_view_position(document);
                needs_redraw = true;
                continue;
            }
        }

        if options.watch_mode {
            if event::poll(WATCH_POLL_INTERVAL)? {
                match event::read()? {
                    Event::Key(key) => app.handle_key(key),
                    Event::Mouse(mouse) => app.handle_mouse(mouse),
                    _ => {}
                }
                needs_redraw = true;
            }
        } else {
            match event::read()? {
                Event::Key(key) => app.handle_key(key),
                Event::Mouse(mouse) => app.handle_mouse(mouse),
                _ => {}
            }
            needs_redraw = true;
        }
    }

    if app.kitty_supported() {
        let mut stdout = io::stdout().lock();
        for command in renderer_state.clear_commands() {
            let command = wrap_command_for_transport(&command, transport);
            stdout.write_all(command.as_bytes())?;
        }
        stdout.flush()?;
    }

    execute!(io::stdout(), DisableMouseCapture)?;

    Ok(())
}

fn maybe_reload_document(
    backend: &PdfBackend,
    session: &mut PdfSession,
    watch_state: &mut FileWatchState,
) -> io::Result<Option<Document>> {
    if !watch_state.has_changed()? {
        return Ok(None);
    }

    let reloaded = match backend.open_session(&watch_state.path) {
        Ok(reloaded) => reloaded,
        Err(_) => return Ok(None),
    };
    let document = reloaded.document().clone();
    *session = reloaded;
    watch_state.refresh_timestamp()?;

    Ok(Some(document))
}

fn maybe_render_page_images(
    app: &mut App,
    session: &mut PdfSession,
    image_area: Rect,
    renderer_state: &mut RendererState,
    transport: KittyTransport,
) -> io::Result<()> {
    let window = terminal::window_size()?;
    let viewport = viewport_pixels(image_area, window);
    app.update_viewport(viewport);

    let frames = if let Some(page_index) = app.presentation_page {
        build_presentation_frame(app, session, viewport, page_index)?
            .into_iter()
            .collect::<Vec<_>>()
    } else {
        build_document_frames(app, session, viewport)?
    };

    let mut stdout = io::stdout().lock();
    for command in renderer_state.prepare_commands(&frames) {
        let command = wrap_command_for_transport(&command, transport);
        stdout.write_all(command.as_bytes())?;
    }
    stdout.flush()?;

    Ok(())
}

fn build_document_frames(
    app: &App,
    session: &mut PdfSession,
    viewport: ViewportPixels,
) -> io::Result<Vec<crate::render::RenderedPage>> {
    let layout = app.document_layout_for(viewport);
    let plans = build_visible_page_plans(&layout, viewport, app.viewport_offset());
    let mut frames = Vec::with_capacity(plans.len());

    for plan in plans {
        let page_bbox = app.document().pages[plan.page_index].bbox;
        let rendered = session
            .render_page(PageRenderPlan {
                page_index: plan.page_index,
                area: plan.area,
                placement_col: plan.placement_col,
                placement_row: plan.placement_row,
                bitmap_width: plan.bitmap_width,
                bitmap_height: plan.bitmap_height,
                crop_x: plan.crop_x,
                crop_y: plan.crop_y,
                crop_width: plan.crop_width,
                crop_height: plan.crop_height,
                placement_columns: plan.placement_columns,
                placement_rows: plan.placement_rows,
            })
            .map_err(io::Error::other)?;

        let passive_highlights = if app.highlight_search() {
            app.match_bounds_for_page(plan.page_index)
        } else {
            Vec::new()
        };
        let active_highlight = if app.highlight_search() {
            app.active_match_bounds_for_page(plan.page_index)
        } else {
            None
        };
        let follow_tags = app.follow_tags_for_page(plan.page_index);

        frames.push(compose_visible_page_frame_with_offsets(
            rendered,
            page_bbox,
            viewport.cell,
            app.dark_mode(),
            &passive_highlights,
            active_highlight,
            &follow_tags,
            Some(FrameOffsets {
                x: u32::from(plan.frame_offset_x),
                y: u32::from(plan.frame_offset_y),
            }),
        ));
    }

    Ok(frames)
}

fn build_presentation_frame(
    app: &App,
    session: &mut PdfSession,
    viewport: ViewportPixels,
    page_index: usize,
) -> io::Result<Option<crate::render::RenderedPage>> {
    let page_bbox = app.document().pages[page_index].bbox;
    let Some(plan) = build_page_render_plan(
        page_index,
        page_bbox,
        viewport,
        app.zoom_factor(),
        ViewportOffset::default(),
    ) else {
        return Ok(None);
    };

    let rendered = session.render_page(plan).map_err(io::Error::other)?;
    let passive_highlights = if app.highlight_search() {
        app.match_bounds_for_page(page_index)
    } else {
        Vec::new()
    };
    let active_highlight = if app.highlight_search() {
        app.active_match_bounds_for_page(page_index)
    } else {
        None
    };
    let follow_tags = app.follow_tags_for_page(page_index);

    Ok(Some(compose_visible_page_frame(
        rendered,
        page_bbox,
        viewport.cell,
        app.dark_mode(),
        &passive_highlights,
        active_highlight,
        &follow_tags,
    )))
}

impl App {
    pub fn new(document: Document) -> Self {
        Self::with_optional_path(document, None)
    }

    pub fn with_path(document: Document, pdf_path: PathBuf) -> Self {
        Self::with_optional_path(document, Some(pdf_path))
    }

    fn with_optional_path(document: Document, pdf_path: Option<PathBuf>) -> Self {
        let index = DocumentIndex::build(&document);
        let mut app = Self {
            document,
            index,
            pdf_path,
            zoom_percent: 100,
            viewport_offset: ViewportOffset::default(),
            viewport: None,
            dark_mode: false,
            search_input: String::new(),
            matches: Vec::new(),
            active_match: None,
            highlight_search: true,
            mode: Mode::Normal,
            status: String::new(),
            kitty_supported: false,
            render_nonce: 0,
            should_quit: false,
            count_buffer: String::new(),
            pending_g: false,
            follow_input: String::new(),
            follow_hints: Vec::new(),
            follow_cluster_rotations: HashMap::new(),
            marks: HashMap::new(),
            presentation_page: None,
        };
        app.status = app.default_status();
        app
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn file_path(&self) -> Option<&Path> {
        self.pdf_path.as_deref()
    }

    pub fn replace_document_preserving_view_position(&mut self, document: Document) {
        let viewport = self.viewport();
        let old_layout = self.document_layout_for(viewport);
        let old_center_x = self.viewport_offset.x.saturating_add(viewport.width / 2);
        let old_center_y = self.viewport_offset.y.saturating_add(viewport.height / 2);
        let old_page_index = self.cursor_page();
        let anchor = old_layout
            .pages
            .get(old_page_index)
            .copied()
            .map(|page_layout| {
                let page_left = page_left_px(viewport.width, page_layout.bitmap_width);
                (
                    old_page_index,
                    relative_position(old_center_x, page_left, page_layout.bitmap_width),
                    relative_position(old_center_y, page_layout.doc_y, page_layout.bitmap_height),
                )
            });

        self.document = document;
        self.index = DocumentIndex::build(&self.document);
        self.matches = self.index.search(&self.search_input);
        self.active_match = self
            .active_match
            .filter(|index| *index < self.matches.len());
        if self.matches.is_empty() {
            self.active_match = None;
        }

        let new_layout = self.document_layout_for(viewport);
        if let Some((page_index, relative_x, relative_y)) = anchor {
            let target_page = page_index.min(self.document.page_count().saturating_sub(1));
            if let Some(page_layout) = new_layout.pages.get(target_page).copied() {
                let page_left = page_left_px(viewport.width, page_layout.bitmap_width);
                let center_x = page_left
                    .saturating_add((relative_x * page_layout.bitmap_width as f32).round() as u32);
                let center_y = page_layout
                    .doc_y
                    .saturating_add((relative_y * page_layout.bitmap_height as f32).round() as u32);
                self.viewport_offset.x = center_x.saturating_sub(viewport.width / 2);
                self.viewport_offset.y = center_y.saturating_sub(viewport.height / 2);
            }
        }
        self.clamp_viewport_offset_with(&new_layout, viewport);

        if self.mode == Mode::Follow {
            self.follow_input.clear();
            self.follow_hints = self.build_follow_hints();
            self.follow_cluster_rotations.clear();
            if self.follow_hints.is_empty() {
                self.mode = Mode::Normal;
            }
        }

        if let Some(page_index) = self.presentation_page {
            let last_page = self.document.page_count().saturating_sub(1);
            self.presentation_page = Some(page_index.min(last_page));
        }

        self.bump_render_nonce();
        self.status = self.default_status();
    }

    pub fn cursor_page(&self) -> usize {
        if let Some(page_index) = self.presentation_page {
            return page_index;
        }

        let viewport = self.viewport();
        let layout = self.document_layout_for(viewport);
        current_page_for_scroll(&layout, self.viewport_offset.y, viewport.height)
    }

    pub fn cursor_line(&self) -> usize {
        if self.presentation_page.is_some() {
            return 0;
        }

        let page_index = self.cursor_page();
        let viewport = self.viewport();
        let layout = self.document_layout_for(viewport);
        let Some(page_layout) = layout.pages.get(page_index).copied() else {
            return 0;
        };
        let page = &self.document.pages[page_index];
        if page.lines.is_empty() {
            return 0;
        }

        let viewport_center = self.viewport_offset.y.saturating_add(viewport.height / 2);
        page.lines
            .iter()
            .enumerate()
            .min_by_key(|(_, line)| {
                let line_center = line_center_y_px(line.bbox, page.bbox, page_layout);
                viewport_center.abs_diff(page_layout.doc_y.saturating_add(line_center))
            })
            .map(|(line_index, _)| line_index)
            .unwrap_or(0)
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn kitty_supported(&self) -> bool {
        self.kitty_supported
    }

    pub fn zoom_factor(&self) -> f32 {
        self.zoom_percent as f32 / 100.0
    }

    pub fn zoom_percent(&self) -> u16 {
        self.zoom_percent
    }

    pub fn dark_mode(&self) -> bool {
        self.dark_mode
    }

    pub fn viewport_offset(&self) -> ViewportOffset {
        self.viewport_offset
    }

    pub fn active_search_match(&self) -> Option<&SearchMatch> {
        self.active_match.and_then(|index| self.matches.get(index))
    }

    pub fn highlight_search(&self) -> bool {
        self.highlight_search
    }

    pub fn active_match_bounds(&self) -> Option<PdfRect> {
        self.active_match_bounds_for_page(self.cursor_page())
    }

    pub fn active_match_bounds_for_page(&self, page: usize) -> Option<PdfRect> {
        self.active_search_match()
            .filter(|matched| matched.page == page)
            .and_then(|matched| self.index.selection_bounds_for_match(matched))
    }

    pub fn current_page_match_bounds(&self) -> Vec<PdfRect> {
        self.match_bounds_for_page(self.cursor_page())
    }

    pub fn match_bounds_for_page(&self, page: usize) -> Vec<PdfRect> {
        self.index
            .selection_bounds_for_page_matches(&self.matches, page)
    }

    pub fn visible_follow_hints(&self) -> Vec<VisibleFollowHint> {
        self.rotated_visible_follow_hints_global()
            .into_iter()
            .map(|hint| VisibleFollowHint {
                label: hint.label.clone(),
                area: hint.area,
            })
            .collect()
    }

    pub fn follow_tags_for_page(&self, page_index: usize) -> Vec<FollowTag> {
        if self.mode != Mode::Follow {
            return Vec::new();
        }

        self.rotated_visible_follow_hints_for_page(page_index)
            .into_iter()
            .filter_map(|hint| {
                self.document.pages[page_index]
                    .links
                    .get(hint.link_index)
                    .map(|link| FollowTag {
                        bounds: link.bbox,
                        label: hint.label.clone(),
                    })
            })
            .collect()
    }

    fn rotated_visible_follow_hints_global(&self) -> Vec<&FollowHint> {
        let matching = self
            .follow_hints
            .iter()
            .filter(|hint| hint.label.starts_with(&self.follow_input))
            .collect::<Vec<_>>();

        rotated_follow_hints_by_clusters(&matching, &self.follow_cluster_rotations)
    }

    fn rotated_visible_follow_hints_for_page(&self, page_index: usize) -> Vec<&FollowHint> {
        let matching = self
            .follow_hints
            .iter()
            .filter(|hint| hint.page == page_index && hint.label.starts_with(&self.follow_input))
            .collect::<Vec<_>>();

        rotated_follow_hints_by_clusters(&matching, &self.follow_cluster_rotations)
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }

        match self.mode {
            Mode::Normal => self.handle_normal_mode(key),
            Mode::Search => self.handle_search_mode(key),
            Mode::Follow => self.handle_follow_mode(key),
            Mode::SetMark => self.handle_mark_mode(key, true),
            Mode::JumpMark => self.handle_mark_mode(key, false),
            Mode::Presentation => self.handle_presentation_mode(key),
        }
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent) {
        if self.mode != Mode::Presentation {
            return;
        }

        if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
            self.advance_presentation(1);
        }
    }

    fn handle_normal_mode(&mut self, key: KeyEvent) {
        if self.pending_g && key.code != KeyCode::Char('g') {
            self.pending_g = false;
        }

        if self.handle_count_prefix(key) {
            return;
        }

        match key.code {
            KeyCode::Esc => self.clear_search_highlight_and_reset(),
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('j') | KeyCode::Down => self.pan_vertical_by_count(1),
            KeyCode::Char('k') | KeyCode::Up => self.pan_vertical_by_count(-1),
            KeyCode::Char('h') | KeyCode::Left => self.pan_horizontal_by_count(-1),
            KeyCode::Char('l') | KeyCode::Right => self.pan_horizontal_by_count(1),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.pan_by_viewport_fraction(1, 2)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.pan_by_viewport_fraction(-1, 2)
            }
            KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.pan_by_viewport_fraction(1, 1)
            }
            KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.pan_by_viewport_fraction(-1, 1)
            }
            KeyCode::Char('g') => self.handle_gg(),
            KeyCode::Char('G') => self.move_to_last_page(),
            KeyCode::Char('i') => self.toggle_dark_mode(),
            KeyCode::Char('+') | KeyCode::Char('=') => self.adjust_zoom(25),
            KeyCode::Char('-') | KeyCode::Char('_') => self.adjust_zoom(-25),
            KeyCode::Char('0') => self.reset_zoom(),
            KeyCode::Char('/') => {
                self.mode = Mode::Search;
                self.search_input.clear();
                self.status = "/".to_string();
            }
            KeyCode::Char('n') => self.advance_match(true),
            KeyCode::Char('N') => self.advance_match(false),
            KeyCode::Char('f') | KeyCode::Char('F') => self.enter_follow_mode(),
            KeyCode::Char('m') => {
                self.mode = Mode::SetMark;
                self.status = "mark: ".to_string();
            }
            KeyCode::Char('`') => {
                self.mode = Mode::JumpMark;
                self.status = "jump mark: ".to_string();
            }
            KeyCode::F(5) => self.enter_presentation_mode(),
            _ => {
                self.count_buffer.clear();
                self.status = self.default_status();
            }
        }
    }

    fn handle_search_mode(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.highlight_search = false;
                self.status = self.default_status();
            }
            KeyCode::Enter => self.submit_search(),
            KeyCode::Backspace => {
                self.search_input.pop();
                self.status = format!("/{}", self.search_input);
            }
            KeyCode::Char(ch) => {
                self.search_input.push(ch);
                self.status = format!("/{}", self.search_input);
            }
            _ => {}
        }
    }

    fn handle_follow_mode(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.follow_input.clear();
                self.follow_hints.clear();
                self.follow_cluster_rotations.clear();
                self.status = self.default_status();
            }
            KeyCode::Char(' ') => {
                self.rotate_follow_hints();
                self.status = self.follow_status();
            }
            KeyCode::Backspace => {
                self.follow_input.pop();
                self.status = self.follow_status();
            }
            KeyCode::Char(ch) if ch.is_ascii_alphanumeric() => {
                self.follow_input.push(ch.to_ascii_lowercase());
                self.status = self.follow_status();
                self.maybe_activate_follow_hint();
            }
            _ => {}
        }
    }

    fn handle_mark_mode(&mut self, key: KeyEvent, set_mark: bool) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.status = self.default_status();
            }
            KeyCode::Char(ch) => {
                if set_mark {
                    self.marks.insert(
                        ch,
                        ViewMark {
                            viewport_offset: self.viewport_offset,
                            zoom_percent: self.zoom_percent,
                        },
                    );
                    self.status = format!("mark '{ch}' set");
                } else if let Some(mark) = self.marks.get(&ch).copied() {
                    self.zoom_percent = mark.zoom_percent;
                    self.viewport_offset = mark.viewport_offset;
                    let viewport = self.viewport();
                    let layout = self.document_layout_for(viewport);
                    self.clamp_viewport_offset_with(&layout, viewport);
                    self.bump_render_nonce();
                    self.status = format!("jumped to mark '{ch}'");
                } else {
                    self.status = format!("mark '{ch}' not set");
                }
                self.mode = Mode::Normal;
            }
            _ => {}
        }
    }

    fn handle_presentation_mode(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::F(5) => self.exit_presentation_mode(),
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Right
            | KeyCode::Down
            | KeyCode::PageDown
            | KeyCode::Enter
            | KeyCode::Char(' ')
            | KeyCode::Char('l')
            | KeyCode::Char('j') => self.advance_presentation(1),
            KeyCode::Left
            | KeyCode::Up
            | KeyCode::PageUp
            | KeyCode::Backspace
            | KeyCode::Char('h')
            | KeyCode::Char('k') => self.advance_presentation(-1),
            _ => {}
        }
    }

    fn handle_count_prefix(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char(ch) if ch.is_ascii_digit() => {
                if ch == '0' && self.count_buffer.is_empty() {
                    return false;
                }
                self.count_buffer.push(ch);
                self.status = self.count_status();
                true
            }
            _ => false,
        }
    }

    fn submit_search(&mut self) {
        self.matches = self.index.search(&self.search_input);
        self.active_match = self.preferred_match_index();
        self.highlight_search = true;
        self.mode = Mode::Normal;
        self.bump_render_nonce();

        if let Some(active) = self.active_match {
            self.jump_to_match(active);
        }

        self.status = self.search_summary();
    }

    fn advance_match(&mut self, forward: bool) {
        if self.matches.is_empty() {
            self.status = "no active search".to_string();
            return;
        }

        let next_index = match self.active_match {
            Some(current) if forward => (current + 1) % self.matches.len(),
            Some(current) => (current + self.matches.len() - 1) % self.matches.len(),
            None => 0,
        };

        self.active_match = Some(next_index);
        self.highlight_search = true;
        self.bump_render_nonce();
        self.jump_to_match(next_index);
        self.status = self.search_summary();
    }

    fn clear_search_highlight_and_reset(&mut self) {
        if self.highlight_search || self.mode == Mode::Search {
            self.highlight_search = false;
        }
        self.mode = Mode::Normal;
        self.follow_input.clear();
        self.follow_hints.clear();
        self.follow_cluster_rotations.clear();
        self.pending_g = false;
        self.count_buffer.clear();
        self.bump_render_nonce();
        self.status = self.default_status();
    }

    fn jump_to_match(&mut self, match_index: usize) {
        if let Some(search_match) = self.matches.get(match_index).cloned() {
            self.move_to(search_match.page, search_match.line);
            self.recenter_viewport_on_match(&search_match);
        }
    }

    fn preferred_match_index(&self) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }

        let current_page = self.cursor_page();

        self.matches
            .iter()
            .enumerate()
            .min_by_key(|(_, matched)| {
                let page_distance = matched.page.abs_diff(current_page);
                let page_penalty = if matched.page == current_page { 0 } else { 1 };
                (page_penalty, page_distance, matched.line)
            })
            .map(|(index, _)| index)
    }

    fn pan_horizontal_by_count(&mut self, direction: i32) {
        let count = self.take_count_or_one();
        let step = VIEWPORT_PAN_STEP_PX.saturating_mul(count);
        self.pan_horizontal(if direction >= 0 {
            step as i32
        } else {
            -(step as i32)
        });
    }

    fn pan_vertical_by_count(&mut self, direction: i32) {
        let count = self.take_count_or_one();
        let step = VIEWPORT_PAN_STEP_PX.saturating_mul(count);
        self.pan_vertical(if direction >= 0 {
            step as i32
        } else {
            -(step as i32)
        });
    }

    fn pan_by_viewport_fraction(&mut self, direction: i32, denominator: u32) {
        let viewport = self.viewport();
        let count = self.take_count_or_one();
        let step = (viewport.height / denominator.max(1)).saturating_mul(count);
        self.pan_vertical(if direction >= 0 {
            step as i32
        } else {
            -(step as i32)
        });
    }

    fn handle_gg(&mut self) {
        if self.pending_g {
            let page = if self.count_buffer.is_empty() {
                0
            } else {
                self.take_count_or_one().saturating_sub(1) as usize
            };
            self.pending_g = false;
            self.move_to(page, 0);
            return;
        }

        self.pending_g = true;
        self.status = if self.count_buffer.is_empty() {
            "g".to_string()
        } else {
            format!("{}g", self.count_buffer)
        };
    }

    fn move_to_last_page(&mut self) {
        self.count_buffer.clear();
        let last_page = self.document.page_count().saturating_sub(1);
        self.move_to(last_page, 0);
    }

    fn move_to(&mut self, page: usize, line: usize) {
        let viewport = self.viewport();
        let layout = self.document_layout_for(viewport);
        let page_index = page.min(self.document.page_count().saturating_sub(1));
        let Some(page_layout) = layout.pages.get(page_index).copied() else {
            return;
        };
        let page = &self.document.pages[page_index];
        let line_index = line.min(page.lines.len().saturating_sub(1));
        let focus_y = page
            .lines
            .get(line_index)
            .map(|line| line_center_y_px(line.bbox, page.bbox, page_layout))
            .unwrap_or(page_layout.bitmap_height / 2);

        self.viewport_offset.y = page_layout
            .doc_y
            .saturating_add(focus_y)
            .saturating_sub(viewport.height / 2)
            .min(self.max_scroll_y_for(&layout, viewport));
        self.clamp_viewport_offset_with(&layout, viewport);
        self.count_buffer.clear();
        self.pending_g = false;
        self.bump_render_nonce();
        self.status = self.default_status();
    }

    fn adjust_zoom(&mut self, delta_percent: i16) {
        let viewport = self.viewport();
        let old_layout = self.document_layout_for(viewport);
        let current_page = self
            .cursor_page()
            .min(self.document.page_count().saturating_sub(1));
        let old_page_layout = old_layout.pages.get(current_page).copied();
        let old_center_x = self.viewport_offset.x.saturating_add(viewport.width / 2);
        let old_center_y = self.viewport_offset.y.saturating_add(viewport.height / 2);

        let next = (self.zoom_percent as i16 + delta_percent).clamp(25, 400);
        self.zoom_percent = next as u16;

        let new_layout = self.document_layout_for(viewport);
        if let (Some(old_page_layout), Some(new_page_layout)) =
            (old_page_layout, new_layout.pages.get(current_page).copied())
        {
            let old_page_left = page_left_px(viewport.width, old_page_layout.bitmap_width);
            let new_page_left = page_left_px(viewport.width, new_page_layout.bitmap_width);
            let old_relative_x =
                relative_position(old_center_x, old_page_left, old_page_layout.bitmap_width);
            let old_relative_y = relative_position(
                old_center_y,
                old_page_layout.doc_y,
                old_page_layout.bitmap_height,
            );
            let new_center_x = new_page_left.saturating_add(
                (old_relative_x * new_page_layout.bitmap_width as f32).round() as u32,
            );
            let new_center_y = new_page_layout.doc_y.saturating_add(
                (old_relative_y * new_page_layout.bitmap_height as f32).round() as u32,
            );

            self.viewport_offset.x = new_center_x.saturating_sub(viewport.width / 2);
            self.viewport_offset.y = new_center_y.saturating_sub(viewport.height / 2);
        }

        self.clamp_viewport_offset_with(&new_layout, viewport);
        self.bump_render_nonce();
        self.status = self.default_status();
    }

    fn reset_zoom(&mut self) {
        self.zoom_percent = 100;
        self.viewport_offset = ViewportOffset::default();
        self.bump_render_nonce();
        self.status = self.default_status();
    }

    fn toggle_dark_mode(&mut self) {
        self.dark_mode = !self.dark_mode;
        self.bump_render_nonce();
        self.status = self.default_status();
    }

    fn pan_horizontal(&mut self, delta: i32) {
        if self.presentation_page.is_some() {
            return;
        }

        let viewport = self.viewport();
        let layout = self.document_layout_for(viewport);
        self.viewport_offset.x = offset_with_delta(
            self.viewport_offset.x,
            delta,
            self.max_scroll_x_for(&layout, viewport),
        );
        self.count_buffer.clear();
        self.bump_render_nonce();
        self.status = self.default_status();
    }

    fn pan_vertical(&mut self, delta: i32) {
        if self.presentation_page.is_some() {
            return;
        }

        let viewport = self.viewport();
        let layout = self.document_layout_for(viewport);
        self.viewport_offset.y = offset_with_delta(
            self.viewport_offset.y,
            delta,
            self.max_scroll_y_for(&layout, viewport),
        );
        self.count_buffer.clear();
        self.bump_render_nonce();
        self.status = self.default_status();
    }

    fn update_viewport(&mut self, viewport: ViewportPixels) {
        self.viewport = Some(viewport);
        let layout = self.document_layout_for(viewport);
        self.clamp_viewport_offset_with(&layout, viewport);
        if self.mode == Mode::Follow {
            self.follow_hints = self.build_follow_hints();
        }
        if matches!(self.mode, Mode::Normal | Mode::Presentation) {
            self.status = self.default_status();
        }
    }

    fn clamp_viewport_offset_with(&mut self, layout: &DocumentLayout, viewport: ViewportPixels) {
        self.viewport_offset.x = self
            .viewport_offset
            .x
            .min(self.max_scroll_x_for(layout, viewport));
        self.viewport_offset.y = self
            .viewport_offset
            .y
            .min(self.max_scroll_y_for(layout, viewport));
    }

    fn recenter_viewport_on_match(&mut self, search_match: &SearchMatch) {
        let viewport = self.viewport();
        let layout = self.document_layout_for(viewport);
        let Some(page_layout) = layout.pages.get(search_match.page).copied() else {
            return;
        };
        let page = &self.document.pages[search_match.page];
        let focus_bbox = self
            .index
            .selection_bounds_for_match(search_match)
            .unwrap_or_else(|| page.lines[search_match.line].bbox);
        let focus_center_x = project_pdf_center_x_to_page(focus_bbox, page.bbox, page_layout);
        let focus_center_y = project_pdf_center_y_to_page(focus_bbox, page.bbox, page_layout);
        let page_left = page_left_px(viewport.width, page_layout.bitmap_width);

        self.viewport_offset.x = page_left
            .saturating_add(focus_center_x)
            .saturating_sub(viewport.width / 2);
        self.viewport_offset.y = page_layout
            .doc_y
            .saturating_add(focus_center_y)
            .saturating_sub(viewport.height / 2);
        self.clamp_viewport_offset_with(&layout, viewport);
        self.bump_render_nonce();
    }

    fn enter_follow_mode(&mut self) {
        self.follow_input.clear();
        self.follow_hints = self.build_follow_hints();
        self.follow_cluster_rotations.clear();
        if self.follow_hints.is_empty() {
            self.status = "no visible links".to_string();
            return;
        }

        self.mode = Mode::Follow;
        self.status = self.follow_status();
    }

    fn maybe_activate_follow_hint(&mut self) {
        let matches = self
            .rotated_visible_follow_hints_global()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();

        if matches.is_empty() {
            self.status = format!("follow: {} (no match)", self.follow_input);
            return;
        }

        if let Some(exact) = matches
            .iter()
            .find(|hint| hint.label == self.follow_input)
            .cloned()
        {
            self.activate_follow_hint(exact);
        }
    }

    fn rotate_follow_hints(&mut self) {
        let matching = self
            .follow_hints
            .iter()
            .filter(|hint| hint.label.starts_with(&self.follow_input))
            .collect::<Vec<_>>();
        let groups = build_follow_overlap_groups(&matching);
        let mut changed = false;

        for group in groups {
            if group.len() <= 1 {
                continue;
            }

            let key = follow_cluster_key(matching[group[0]]);
            let entry = self.follow_cluster_rotations.entry(key).or_insert(0);
            *entry = (*entry + 1) % group.len();
            changed = true;
        }

        if changed {
            self.bump_render_nonce();
        }
    }

    fn activate_follow_hint(&mut self, hint: FollowHint) {
        let Some(link) = self
            .document
            .pages
            .get(hint.page)
            .and_then(|page| page.links.get(hint.link_index))
            .cloned()
        else {
            self.status = "link target unavailable".to_string();
            self.mode = Mode::Normal;
            self.follow_input.clear();
            self.follow_hints.clear();
            self.follow_cluster_rotations.clear();
            return;
        };

        self.mode = Mode::Normal;
        self.follow_input.clear();
        self.follow_hints.clear();
        self.follow_cluster_rotations.clear();

        match link.target {
            LinkTarget::LocalDestination { page, x, y, .. } => {
                self.jump_to_link_destination(page, x, y);
                self.status = self.default_status();
            }
            LinkTarget::ExternalUri(uri) => {
                self.status = if open_external_uri(&uri).is_ok() {
                    format!("opened {uri}")
                } else {
                    format!("failed to open {uri}")
                };
            }
        }
    }

    fn jump_to_link_destination(&mut self, page_index: usize, x: Option<f32>, y: Option<f32>) {
        let viewport = self.viewport();
        let layout = self.document_layout_for(viewport);
        let Some(page_layout) = layout.pages.get(page_index).copied() else {
            return;
        };
        let page = &self.document.pages[page_index];
        let page_left = page_left_px(viewport.width, page_layout.bitmap_width);
        let target_x = x
            .map(|value| {
                ((value / page.bbox.width.max(1.0)) * page_layout.bitmap_width as f32) as u32
            })
            .unwrap_or(page_layout.bitmap_width / 2);
        let target_y = y
            .map(|value| {
                ((page.bbox.height - value).max(0.0) / page.bbox.height.max(1.0)
                    * page_layout.bitmap_height as f32) as u32
            })
            .unwrap_or(0);

        self.viewport_offset.x = page_left
            .saturating_add(target_x)
            .saturating_sub(viewport.width / 2);
        self.viewport_offset.y = page_layout
            .doc_y
            .saturating_add(target_y)
            .saturating_sub(viewport.height / 2);
        self.clamp_viewport_offset_with(&layout, viewport);
        self.bump_render_nonce();
    }

    fn enter_presentation_mode(&mut self) {
        self.presentation_page = Some(self.cursor_page());
        self.mode = Mode::Presentation;
        self.status = self.default_status();
    }

    fn exit_presentation_mode(&mut self) {
        let Some(page_index) = self.presentation_page.take() else {
            return;
        };

        self.mode = Mode::Normal;
        self.move_to(page_index, 0);
    }

    fn advance_presentation(&mut self, delta: isize) {
        let Some(current) = self.presentation_page else {
            return;
        };

        let max_page = self.document.page_count().saturating_sub(1) as isize;
        let next = (current as isize + delta).clamp(0, max_page) as usize;
        self.presentation_page = Some(next);
        self.bump_render_nonce();
        self.status = self.default_status();
    }

    fn build_follow_hints(&self) -> Vec<FollowHint> {
        let viewport = self.viewport();
        let layout = self.document_layout_for(viewport);
        let plans = build_visible_page_plans(&layout, viewport, self.viewport_offset());
        let mut hints = Vec::new();

        for plan in plans {
            let page = &self.document.pages[plan.page_index];
            for (link_index, link) in page.links.iter().enumerate() {
                if let Some(area) = project_link_to_cells(plan, page.bbox, link, viewport.cell) {
                    hints.push((plan.page_index, link_index, area));
                }
            }
        }

        generate_hint_labels(hints.len())
            .into_iter()
            .zip(hints)
            .map(|(label, (page, link_index, area))| FollowHint {
                label,
                page,
                link_index,
                area,
            })
            .collect()
    }

    fn take_count_or_one(&mut self) -> u32 {
        let count = self.count_buffer.parse::<u32>().unwrap_or(1).max(1);
        self.count_buffer.clear();
        count
    }

    fn max_scroll_x_for(&self, layout: &DocumentLayout, viewport: ViewportPixels) -> u32 {
        layout
            .pages
            .iter()
            .map(|page| page.bitmap_width.saturating_sub(viewport.width))
            .max()
            .unwrap_or(0)
    }

    fn max_scroll_y_for(&self, layout: &DocumentLayout, viewport: ViewportPixels) -> u32 {
        layout.total_height.saturating_sub(viewport.height)
    }

    fn viewport(&self) -> ViewportPixels {
        self.viewport.unwrap_or(ViewportPixels {
            area: DEFAULT_VIEWPORT_AREA,
            cell: DEFAULT_CELL,
            width: u32::from(DEFAULT_VIEWPORT_AREA.width) * u32::from(DEFAULT_CELL.width),
            height: u32::from(DEFAULT_VIEWPORT_AREA.height) * u32::from(DEFAULT_CELL.height),
        })
    }

    fn document_layout_for(&self, viewport: ViewportPixels) -> DocumentLayout {
        build_document_layout(&self.document, viewport, self.zoom_factor())
    }

    fn search_summary(&self) -> String {
        if self.matches.is_empty() {
            if self.search_input.is_empty() {
                return self.default_status();
            }

            return format!("0 matches for /{}", self.search_input);
        }

        let active = self.active_match.unwrap_or(0) + 1;
        format!(
            "match {active}/{} for /{}",
            self.matches.len(),
            self.search_input
        )
    }

    fn count_status(&self) -> String {
        format!("count: {}", self.count_buffer)
    }

    fn follow_status(&self) -> String {
        format!("follow: {}", self.follow_input)
    }

    fn default_status(&self) -> String {
        if let Some(page_index) = self.presentation_page {
            return format!(
                "page {}/{} | zoom {}% | presentation | click/Space next | Backspace prev | F5/Esc exit",
                page_index + 1,
                self.document.page_count(),
                self.zoom_percent,
            );
        }

        let page = self.cursor_page();
        format!(
            "line {}/{} | zoom {}%",
            self.cursor_line() + 1,
            self.document.pages[page].lines.len().max(1),
            self.zoom_percent,
        )
    }

    fn bump_render_nonce(&mut self) {
        self.render_nonce = self.render_nonce.wrapping_add(1);
    }
}

impl FileWatchState {
    fn from_path(path: &Path) -> Self {
        let path = path.to_path_buf();
        let last_modified = file_modified_time(&path).ok().flatten();
        Self {
            path,
            last_modified,
        }
    }

    fn has_changed(&self) -> io::Result<bool> {
        Ok(file_modified_time(&self.path)? != self.last_modified)
    }

    fn refresh_timestamp(&mut self) -> io::Result<()> {
        self.last_modified = file_modified_time(&self.path)?;
        Ok(())
    }
}

fn file_modified_time(path: &Path) -> io::Result<Option<SystemTime>> {
    match std::fs::metadata(path) {
        Ok(metadata) => metadata.modified().map(Some),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn offset_with_delta(current: u32, delta: i32, max: u32) -> u32 {
    if delta >= 0 {
        current.saturating_add(delta as u32).min(max)
    } else {
        current.saturating_sub(delta.unsigned_abs()).min(max)
    }
}

fn page_left_px(viewport_width: u32, page_width: u32) -> u32 {
    viewport_width.saturating_sub(page_width) / 2
}

fn relative_position(center: u32, start: u32, extent: u32) -> f32 {
    if extent == 0 {
        return 0.5;
    }

    center.saturating_sub(start) as f32 / extent as f32
}

fn project_pdf_center_x_to_page(
    bounds: PdfRect,
    page_bbox: PdfRect,
    page_layout: DocumentLayoutPage,
) -> u32 {
    let scale_x = page_layout.bitmap_width as f32 / page_bbox.width.max(1.0);
    ((bounds.x + bounds.width / 2.0) * scale_x)
        .round()
        .clamp(0.0, page_layout.bitmap_width as f32) as u32
}

fn project_pdf_center_y_to_page(
    bounds: PdfRect,
    page_bbox: PdfRect,
    page_layout: DocumentLayoutPage,
) -> u32 {
    let scale_y = page_layout.bitmap_height as f32 / page_bbox.height.max(1.0);
    let projected = (page_bbox.height - (bounds.y + bounds.height / 2.0)).max(0.0) * scale_y;
    projected
        .round()
        .clamp(0.0, page_layout.bitmap_height as f32) as u32
}

fn line_center_y_px(
    line_bbox: PdfRect,
    page_bbox: PdfRect,
    page_layout: DocumentLayoutPage,
) -> u32 {
    project_pdf_center_y_to_page(line_bbox, page_bbox, page_layout)
}

fn project_link_to_cells(
    plan: crate::render::VisiblePagePlan,
    page_bbox: PdfRect,
    link: &PageLink,
    cell: CellPixels,
) -> Option<Rect> {
    let scale_x = plan.bitmap_width as f32 / page_bbox.width.max(1.0);
    let scale_y = plan.bitmap_height as f32 / page_bbox.height.max(1.0);
    let projected_top = (page_bbox.height - (link.bbox.y + link.bbox.height)).max(0.0);
    let projected_bottom = (page_bbox.height - link.bbox.y).max(0.0);

    let left = (link.bbox.x * scale_x).floor() as i32 - plan.crop_x as i32;
    let top = (projected_top * scale_y).floor() as i32 - plan.crop_y as i32;
    let right = ((link.bbox.x + link.bbox.width) * scale_x).ceil() as i32 - plan.crop_x as i32;
    let bottom = (projected_bottom * scale_y).ceil() as i32 - plan.crop_y as i32;

    let clamped_left = left.max(0) as u32;
    let clamped_top = top.max(0) as u32;
    let clipped_right = right.max(0) as u32;
    let clipped_bottom = bottom.max(0) as u32;
    if clamped_left >= clipped_right || clamped_top >= clipped_bottom {
        return None;
    }

    let pixel_x = u32::from(plan.frame_offset_x).saturating_add(clamped_left);
    let pixel_y = u32::from(plan.frame_offset_y).saturating_add(clamped_top);
    let width = clipped_right.saturating_sub(clamped_left).max(1);
    let height = clipped_bottom.saturating_sub(clamped_top).max(1);

    Some(Rect::new(
        plan.placement_col + (pixel_x / u32::from(cell.width.max(1))) as u16,
        plan.placement_row + (pixel_y / u32::from(cell.height.max(1))) as u16,
        ((u32::from(pixel_x as u16 % cell.width.max(1)) + width)
            .div_ceil(u32::from(cell.width.max(1)))) as u16,
        ((u32::from(pixel_y as u16 % cell.height.max(1)) + height)
            .div_ceil(u32::from(cell.height.max(1)))) as u16,
    ))
}

fn build_follow_overlap_groups(hints: &[&FollowHint]) -> Vec<Vec<usize>> {
    if hints.is_empty() {
        return Vec::new();
    }

    let mut groups = Vec::new();
    let mut assigned = vec![false; hints.len()];

    for start in 0..hints.len() {
        if assigned[start] {
            continue;
        }

        let mut group = vec![start];
        let mut shared_rect = follow_hint_badge_rect(hints[start]);
        assigned[start] = true;

        let mut changed = true;
        while changed {
            changed = false;
            for candidate in 0..hints.len() {
                if assigned[candidate] {
                    continue;
                }
                if let Some(intersection) =
                    rect_intersection(shared_rect, follow_hint_badge_rect(hints[candidate]))
                {
                    assigned[candidate] = true;
                    group.push(candidate);
                    shared_rect = intersection;
                    changed = true;
                }
            }
        }

        group.sort_unstable();
        groups.push(group);
    }

    groups
}

fn rotated_follow_hints_by_clusters<'a>(
    hints: &[&'a FollowHint],
    rotations: &HashMap<FollowClusterKey, usize>,
) -> Vec<&'a FollowHint> {
    if hints.is_empty() {
        return Vec::new();
    }

    let overlap_groups = build_follow_overlap_groups(hints);
    let mut ordered = Vec::with_capacity(hints.len());

    for group in overlap_groups {
        if group.len() <= 1 {
            ordered.push(hints[group[0]]);
            continue;
        }

        let key = follow_cluster_key(hints[group[0]]);
        let rotation = rotations.get(&key).copied().unwrap_or(0) % group.len();
        ordered.extend(
            group
                .iter()
                .cycle()
                .skip(rotation)
                .take(group.len())
                .map(|index| hints[*index]),
        );
    }

    ordered
}

fn follow_cluster_key(hint: &FollowHint) -> FollowClusterKey {
    FollowClusterKey {
        page: hint.page,
        first_link_index: hint.link_index,
    }
}

fn rect_intersection(left_rect: Rect, right_rect: Rect) -> Option<Rect> {
    let left = left_rect.x.max(right_rect.x);
    let top = left_rect.y.max(right_rect.y);
    let right = left_rect
        .x
        .saturating_add(left_rect.width)
        .min(right_rect.x.saturating_add(right_rect.width));
    let bottom = left_rect
        .y
        .saturating_add(left_rect.height)
        .min(right_rect.y.saturating_add(right_rect.height));
    if left < right && top < bottom {
        Some(Rect::new(left, top, right - left, bottom - top))
    } else {
        None
    }
}

fn follow_hint_badge_rect(hint: &FollowHint) -> Rect {
    let (width, height) = follow_tag_badge_size(&hint.label);
    Rect::new(hint.area.x, hint.area.y, width as u16, height as u16)
}

fn generate_hint_labels(count: usize) -> Vec<String> {
    (0..count).map(label_for_index).collect()
}

fn label_for_index(mut index: usize) -> String {
    let mut label = String::new();
    loop {
        let rem = index % 26;
        label.insert(0, (b'a' + rem as u8) as char);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    label
}

fn open_external_uri(uri: &str) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(uri);
        command
    };

    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(uri);
        command
    };

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", uri]);
        command
    };

    command.spawn()?.wait()?;
    Ok(())
}
