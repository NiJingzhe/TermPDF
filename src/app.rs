use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal;
use ratatui::DefaultTerminal;
use ratatui::layout::Rect;

use crate::document::{Document, LinkTarget, PageLink, PdfRect};
use crate::kitty::{KittyTransport, RendererState, wrap_command_for_transport};
use crate::pdf::{PdfBackend, PdfSession};
use crate::platform::{kitty_transport, likely_supports_kitty_graphics};
use crate::render::{
    CellPixels, DocumentLayout, DocumentLayoutPage, FollowTag, FrameOffsets, PageRenderPlan,
    ViewportOffset, ViewportPixels, build_document_layout, build_page_render_plan,
    build_visible_page_plans, compose_visible_page_frame, compose_visible_page_frame_with_offsets,
    current_page_for_scroll, follow_tag_badge_size, viewport_pixels,
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
const MOUSE_SCROLL_ROWS: u32 = 3;
const MOUSE_SCROLL_COLUMNS: u32 = 6;
const MOUSE_ZOOM_STEP_PERCENT: i16 = 10;
const COALESCED_ZOOM_LIMIT_PERCENT: i16 = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunOptions {
    pub watch_mode: bool,
}

impl RunOptions {
    pub const fn new(watch_mode: bool) -> Self {
        Self { watch_mode }
    }
}

impl ClipboardBackend {
    fn memory() -> (Self, ClipboardCapture) {
        let capture = ClipboardCapture {
            text: Rc::new(RefCell::new(None)),
        };
        (Self::Memory(capture.clone()), capture)
    }

    fn copy(&self, text: &str) -> io::Result<()> {
        match self {
            Self::System => copy_to_system_clipboard(text),
            Self::Memory(capture) => {
                *capture.text.borrow_mut() = Some(text.to_string());
                Ok(())
            }
        }
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
    Visual,
    VisualLine,
    Presentation,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextCursor {
    pub page: usize,
    pub line: usize,
    pub glyph: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextSelectionRange {
    pub page: usize,
    pub line: usize,
    pub start_glyph: usize,
    pub end_glyph: usize,
}

#[derive(Clone, Debug)]
pub struct ClipboardCapture {
    text: Rc<RefCell<Option<String>>>,
}

impl ClipboardCapture {
    pub fn text(&self) -> Option<String> {
        self.text.borrow().clone()
    }
}

#[derive(Clone, Debug)]
enum ClipboardBackend {
    System,
    Memory(ClipboardCapture),
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct NavigationBatch {
    pan_x: i32,
    pan_y: i32,
    zoom: i16,
    zoom_mouse: Option<MouseEvent>,
    presentation_delta: isize,
}

impl NavigationBatch {
    fn add_pan_x(&mut self, delta: i32) {
        self.pan_x = coalesce_axis_i32(self.pan_x, delta);
    }

    fn add_pan_y(&mut self, delta: i32) {
        self.pan_y = coalesce_axis_i32(self.pan_y, delta);
    }

    fn add_zoom(&mut self, delta: i16, mouse: Option<MouseEvent>) {
        self.zoom = coalesce_axis_i16(self.zoom, delta)
            .clamp(-COALESCED_ZOOM_LIMIT_PERCENT, COALESCED_ZOOM_LIMIT_PERCENT);
        self.zoom_mouse = if self.zoom == 0 { None } else { mouse };
    }

    fn add_presentation_delta(&mut self, delta: isize) {
        self.presentation_delta = coalesce_axis_isize(self.presentation_delta, delta);
    }
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
    text_cursor: Option<TextCursor>,
    visual_anchor: Option<TextCursor>,
    clipboard: ClipboardBackend,
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
    let transport = kitty_transport();

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

        if let Some(watch_state) = watch_state.as_mut()
            && let Some(document) = maybe_reload_document(backend, session, watch_state)?
        {
            app.replace_document_preserving_view_position(document);
            needs_redraw = true;
            continue;
        }

        if options.watch_mode {
            if event::poll(WATCH_POLL_INTERVAL)? {
                let first = event::read()?;
                app.handle_events(read_event_batch(first)?);
                needs_redraw = true;
            }
        } else {
            let first = event::read()?;
            app.handle_events(read_event_batch(first)?);
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

fn read_event_batch(first: Event) -> io::Result<Vec<Event>> {
    let mut events = vec![first];
    while event::poll(Duration::ZERO)? {
        events.push(event::read()?);
    }
    Ok(events)
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
        let selection_highlights = app.selection_bounds_for_page(plan.page_index);
        let cursor_highlight = app.cursor_bounds_for_page(plan.page_index);

        frames.push(compose_visible_page_frame_with_offsets(
            rendered,
            page_bbox,
            viewport.cell,
            app.dark_mode(),
            &passive_highlights,
            active_highlight,
            &selection_highlights,
            cursor_highlight,
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
    let selection_highlights = app.selection_bounds_for_page(page_index);
    let cursor_highlight = app.cursor_bounds_for_page(page_index);

    Ok(Some(compose_visible_page_frame(
        rendered,
        page_bbox,
        viewport.cell,
        app.dark_mode(),
        &passive_highlights,
        active_highlight,
        &selection_highlights,
        cursor_highlight,
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
            text_cursor: None,
            visual_anchor: None,
            clipboard: ClipboardBackend::System,
        };
        app.status = app.default_status();
        app
    }

    pub fn with_memory_clipboard_for_tests(document: Document) -> (Self, ClipboardCapture) {
        let (clipboard, capture) = ClipboardBackend::memory();
        let mut app = Self::with_optional_path(document, None);
        app.clipboard = clipboard;
        (app, capture)
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

        if matches!(self.mode, Mode::Visual | Mode::VisualLine) {
            self.mode = Mode::Normal;
            self.visual_anchor = None;
            self.text_cursor = None;
        } else {
            self.text_cursor = self
                .text_cursor
                .map(|cursor| self.clamp_text_cursor(cursor));
        }

        self.bump_render_nonce();
        self.status = self.default_status();
    }

    pub fn cursor_page(&self) -> usize {
        if let Some(page_index) = self.presentation_page {
            return page_index;
        }
        if let Some(cursor) = self.text_cursor {
            return self.clamp_text_cursor(cursor).page;
        }

        let viewport = self.viewport();
        let layout = self.document_layout_for(viewport);
        current_page_for_scroll(&layout, self.viewport_offset.y, viewport.height)
    }

    pub fn cursor_line(&self) -> usize {
        if self.presentation_page.is_some() {
            return 0;
        }
        if let Some(cursor) = self.text_cursor {
            return self.clamp_text_cursor(cursor).line;
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

    pub fn text_cursor(&self) -> TextCursor {
        self.current_text_cursor()
    }

    pub fn selection_ranges(&self) -> Vec<TextSelectionRange> {
        self.visual_selection_ranges()
    }

    pub fn selected_text(&self) -> Option<String> {
        self.visual_selected_text()
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

    pub fn selection_bounds_for_page(&self, page: usize) -> Vec<PdfRect> {
        self.selection_ranges()
            .into_iter()
            .filter(|range| range.page == page)
            .filter_map(|range| self.selection_range_bounds(range))
            .collect()
    }

    pub fn cursor_bounds_for_page(&self, page: usize) -> Option<PdfRect> {
        if matches!(self.mode, Mode::Visual | Mode::VisualLine) {
            return None;
        }

        let cursor = self.current_text_cursor();
        if cursor.page != page {
            return None;
        }

        self.document
            .pages
            .get(cursor.page)
            .and_then(|page| page.lines.get(cursor.line))
            .and_then(|line| line.glyphs.get(cursor.glyph).or_else(|| line.glyphs.last()))
            .map(|glyph| glyph.bbox)
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
            Mode::Visual | Mode::VisualLine => self.handle_visual_mode(key),
            Mode::Presentation => self.handle_presentation_mode(key),
        }
    }

    pub fn handle_events<I>(&mut self, events: I)
    where
        I: IntoIterator<Item = Event>,
    {
        let mut batch = NavigationBatch::default();

        for event in events {
            match event {
                Event::Key(key) => {
                    if !self.try_accumulate_key_navigation(key, &mut batch) {
                        self.apply_navigation_batch(batch);
                        batch = NavigationBatch::default();
                        self.handle_key(key);
                    }
                }
                Event::Mouse(mouse) => {
                    if !self.try_accumulate_mouse_navigation(mouse, &mut batch) {
                        self.apply_navigation_batch(batch);
                        batch = NavigationBatch::default();
                        self.handle_mouse(mouse);
                    }
                }
                _ => {}
            }
        }

        self.apply_navigation_batch(batch);
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent) {
        match self.mode {
            Mode::Normal => self.handle_normal_mouse(mouse),
            Mode::Presentation => self.handle_presentation_mouse(mouse),
            _ => {}
        }
    }

    fn try_accumulate_key_navigation(
        &mut self,
        key: KeyEvent,
        batch: &mut NavigationBatch,
    ) -> bool {
        if key.kind != KeyEventKind::Press {
            return true;
        }

        match self.mode {
            Mode::Normal if self.pending_g || !self.count_buffer.is_empty() => false,
            Mode::Normal => match key.code {
                KeyCode::Char('J') => {
                    batch.add_pan_y(VIEWPORT_PAN_STEP_PX as i32);
                    true
                }
                KeyCode::Char('K') => {
                    batch.add_pan_y(-(VIEWPORT_PAN_STEP_PX as i32));
                    true
                }
                KeyCode::Char('H') => {
                    batch.add_pan_x(-(VIEWPORT_PAN_STEP_PX as i32));
                    true
                }
                KeyCode::Char('L') => {
                    batch.add_pan_x(VIEWPORT_PAN_STEP_PX as i32);
                    true
                }
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    batch.add_pan_y(self.viewport().height.min(i32::MAX as u32) as i32 / 2);
                    true
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    batch.add_pan_y(-(self.viewport().height.min(i32::MAX as u32) as i32 / 2));
                    true
                }
                KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    batch.add_pan_y(self.viewport().height.min(i32::MAX as u32) as i32);
                    true
                }
                KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    batch.add_pan_y(-(self.viewport().height.min(i32::MAX as u32) as i32));
                    true
                }
                KeyCode::Char('+') | KeyCode::Char('=') => {
                    batch.add_zoom(25, None);
                    true
                }
                KeyCode::Char('-') | KeyCode::Char('_') => {
                    batch.add_zoom(-25, None);
                    true
                }
                _ => false,
            },
            Mode::Presentation => match key.code {
                KeyCode::Right
                | KeyCode::Down
                | KeyCode::PageDown
                | KeyCode::Enter
                | KeyCode::Char(' ')
                | KeyCode::Char('l')
                | KeyCode::Char('j') => {
                    batch.add_presentation_delta(1);
                    true
                }
                KeyCode::Left
                | KeyCode::Up
                | KeyCode::PageUp
                | KeyCode::Backspace
                | KeyCode::Char('h')
                | KeyCode::Char('k') => {
                    batch.add_presentation_delta(-1);
                    true
                }
                _ => false,
            },
            _ => false,
        }
    }

    fn try_accumulate_mouse_navigation(
        &mut self,
        mouse: MouseEvent,
        batch: &mut NavigationBatch,
    ) -> bool {
        match self.mode {
            Mode::Normal => match mouse.kind {
                MouseEventKind::ScrollUp if mouse.modifiers.contains(KeyModifiers::CONTROL) => {
                    batch.add_zoom(MOUSE_ZOOM_STEP_PERCENT, Some(mouse));
                    true
                }
                MouseEventKind::ScrollDown if mouse.modifiers.contains(KeyModifiers::CONTROL) => {
                    batch.add_zoom(-MOUSE_ZOOM_STEP_PERCENT, Some(mouse));
                    true
                }
                MouseEventKind::ScrollUp if mouse.modifiers.contains(KeyModifiers::SHIFT) => {
                    batch.add_pan_x(-(self.mouse_horizontal_scroll_step() as i32));
                    true
                }
                MouseEventKind::ScrollDown if mouse.modifiers.contains(KeyModifiers::SHIFT) => {
                    batch.add_pan_x(self.mouse_horizontal_scroll_step() as i32);
                    true
                }
                MouseEventKind::ScrollUp => {
                    batch.add_pan_y(-(self.mouse_vertical_scroll_step() as i32));
                    true
                }
                MouseEventKind::ScrollDown => {
                    batch.add_pan_y(self.mouse_vertical_scroll_step() as i32);
                    true
                }
                MouseEventKind::ScrollLeft => {
                    batch.add_pan_x(-(self.mouse_horizontal_scroll_step() as i32));
                    true
                }
                MouseEventKind::ScrollRight => {
                    batch.add_pan_x(self.mouse_horizontal_scroll_step() as i32);
                    true
                }
                _ => false,
            },
            Mode::Presentation => match mouse.kind {
                MouseEventKind::Down(MouseButton::Left)
                | MouseEventKind::ScrollDown
                | MouseEventKind::ScrollRight => {
                    batch.add_presentation_delta(1);
                    true
                }
                MouseEventKind::ScrollUp | MouseEventKind::ScrollLeft => {
                    batch.add_presentation_delta(-1);
                    true
                }
                _ => false,
            },
            _ => false,
        }
    }

    fn apply_navigation_batch(&mut self, batch: NavigationBatch) {
        if batch.pan_x != 0 {
            self.pan_horizontal(batch.pan_x);
        }
        if batch.pan_y != 0 {
            self.pan_vertical(batch.pan_y);
        }
        if batch.zoom != 0 {
            if let Some(mouse) = batch.zoom_mouse {
                self.adjust_zoom_at_mouse(mouse, batch.zoom);
            } else {
                self.adjust_zoom(batch.zoom);
            }
        }
        if batch.presentation_delta != 0 {
            self.advance_presentation(batch.presentation_delta);
        }
    }

    fn handle_normal_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollUp if mouse.modifiers.contains(KeyModifiers::CONTROL) => {
                self.adjust_zoom_at_mouse(mouse, MOUSE_ZOOM_STEP_PERCENT)
            }
            MouseEventKind::ScrollDown if mouse.modifiers.contains(KeyModifiers::CONTROL) => {
                self.adjust_zoom_at_mouse(mouse, -MOUSE_ZOOM_STEP_PERCENT)
            }
            MouseEventKind::ScrollUp if mouse.modifiers.contains(KeyModifiers::SHIFT) => {
                self.pan_horizontal(-(self.mouse_horizontal_scroll_step() as i32))
            }
            MouseEventKind::ScrollDown if mouse.modifiers.contains(KeyModifiers::SHIFT) => {
                self.pan_horizontal(self.mouse_horizontal_scroll_step() as i32)
            }
            MouseEventKind::ScrollUp => {
                self.pan_vertical(-(self.mouse_vertical_scroll_step() as i32))
            }
            MouseEventKind::ScrollDown => {
                self.pan_vertical(self.mouse_vertical_scroll_step() as i32)
            }
            MouseEventKind::ScrollLeft => {
                self.pan_horizontal(-(self.mouse_horizontal_scroll_step() as i32))
            }
            MouseEventKind::ScrollRight => {
                self.pan_horizontal(self.mouse_horizontal_scroll_step() as i32)
            }
            _ => {}
        }
    }

    fn handle_presentation_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left)
            | MouseEventKind::ScrollDown
            | MouseEventKind::ScrollRight => self.advance_presentation(1),
            MouseEventKind::ScrollUp | MouseEventKind::ScrollLeft => self.advance_presentation(-1),
            _ => {}
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
            KeyCode::Char('j') | KeyCode::Down => self.move_text_cursor_vertical_by_count(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_text_cursor_vertical_by_count(-1),
            KeyCode::Char('h') | KeyCode::Left => self.move_text_cursor_horizontal_by_count(-1),
            KeyCode::Char('l') | KeyCode::Right => self.move_text_cursor_horizontal_by_count(1),
            KeyCode::Char('J') => self.pan_vertical_by_count(1),
            KeyCode::Char('K') => self.pan_vertical_by_count(-1),
            KeyCode::Char('H') => self.pan_horizontal_by_count(-1),
            KeyCode::Char('L') => self.pan_horizontal_by_count(1),
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
            KeyCode::Char('w') => self.move_text_cursor_word_forward_by_count(),
            KeyCode::Char('b') => self.move_text_cursor_word_backward_by_count(),
            KeyCode::Char('$') => self.move_text_cursor_to_line_end(),
            KeyCode::Char('^') => self.move_text_cursor_to_first_non_whitespace(),
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
            KeyCode::Char('v') => self.enter_visual_mode(Mode::Visual),
            KeyCode::Char('V') => self.enter_visual_mode(Mode::VisualLine),
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

    fn enter_visual_mode(&mut self, mode: Mode) {
        let cursor = self.current_text_cursor();
        self.mode = mode;
        self.text_cursor = Some(cursor);
        self.visual_anchor = Some(cursor);
        self.pending_g = false;
        self.count_buffer.clear();
        self.bump_render_nonce();
        self.status = self.visual_status();
    }

    fn handle_visual_mode(&mut self, key: KeyEvent) {
        if self.handle_count_prefix(key) {
            return;
        }

        match key.code {
            KeyCode::Esc => self.exit_visual_mode(self.default_status()),
            KeyCode::Char('v') => {
                if self.mode == Mode::Visual {
                    self.exit_visual_mode(self.default_status());
                } else {
                    self.mode = Mode::Visual;
                    self.status = self.visual_status();
                    self.bump_render_nonce();
                }
            }
            KeyCode::Char('V') => {
                if self.mode == Mode::VisualLine {
                    self.exit_visual_mode(self.default_status());
                } else {
                    self.mode = Mode::VisualLine;
                    self.status = self.visual_status();
                    self.bump_render_nonce();
                }
            }
            KeyCode::Char('y') => self.yank_visual_selection(),
            KeyCode::Char('h') | KeyCode::Left => self.move_text_cursor_horizontal_by_count(-1),
            KeyCode::Char('l') | KeyCode::Right => self.move_text_cursor_horizontal_by_count(1),
            KeyCode::Char('j') | KeyCode::Down => self.move_text_cursor_vertical_by_count(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_text_cursor_vertical_by_count(-1),
            KeyCode::Char('w') => self.move_text_cursor_word_forward_by_count(),
            KeyCode::Char('b') => self.move_text_cursor_word_backward_by_count(),
            KeyCode::Char('$') => self.move_text_cursor_to_line_end(),
            KeyCode::Char('^') => self.move_text_cursor_to_first_non_whitespace(),
            _ => {}
        }
    }

    fn exit_visual_mode(&mut self, status: String) {
        let cursor = self
            .text_cursor
            .map(|cursor| self.clamp_text_cursor(cursor));
        self.mode = Mode::Normal;
        self.visual_anchor = None;
        self.text_cursor = cursor;
        self.pending_g = false;
        self.count_buffer.clear();
        self.bump_render_nonce();
        self.status = status;
    }

    fn yank_visual_selection(&mut self) {
        let Some(text) = self.visual_selected_text() else {
            self.exit_visual_mode("nothing selected".to_string());
            return;
        };
        let char_count = text.chars().count();
        match self.clipboard.copy(&text) {
            Ok(()) => self.exit_visual_mode(format!("copied {char_count} chars")),
            Err(error) => self.exit_visual_mode(format!("copy failed: {error}")),
        }
    }

    fn move_text_cursor_horizontal(&mut self, delta: isize) {
        let mut cursor = self.current_text_cursor();
        let glyph_count = self.line_glyph_count(cursor.page, cursor.line);
        if glyph_count == 0 {
            cursor.glyph = 0;
        } else if delta < 0 {
            cursor.glyph = cursor.glyph.saturating_sub(delta.unsigned_abs());
        } else {
            cursor.glyph = cursor
                .glyph
                .saturating_add(delta as usize)
                .min(glyph_count - 1);
        }
        self.set_text_cursor(cursor);
    }

    fn move_text_cursor_horizontal_by_count(&mut self, direction: isize) {
        let count = self.take_count_or_one() as isize;
        self.move_text_cursor_horizontal(direction.saturating_mul(count));
    }

    fn move_text_cursor_vertical(&mut self, delta: isize) {
        let mut cursor = self.current_text_cursor();
        let steps = delta.unsigned_abs();
        for _ in 0..steps {
            cursor = if delta >= 0 {
                self.next_line_cursor(cursor)
            } else {
                self.previous_line_cursor(cursor)
            };
        }
        self.set_text_cursor(cursor);
    }

    fn move_text_cursor_vertical_by_count(&mut self, direction: isize) {
        let count = self.take_count_or_one() as isize;
        self.move_text_cursor_vertical(direction.saturating_mul(count));
    }

    fn move_text_cursor_word_forward_by_count(&mut self) {
        let count = self.take_count_or_one();
        let mut cursor = self.current_text_cursor();
        for _ in 0..count {
            cursor = self.next_word_start_cursor(cursor);
        }
        self.set_text_cursor(cursor);
    }

    fn move_text_cursor_word_backward_by_count(&mut self) {
        let count = self.take_count_or_one();
        let mut cursor = self.current_text_cursor();
        for _ in 0..count {
            cursor = self.previous_word_start_cursor(cursor);
        }
        self.set_text_cursor(cursor);
    }

    fn move_text_cursor_to_line_end(&mut self) {
        self.count_buffer.clear();
        let mut cursor = self.current_text_cursor();
        cursor.glyph = self
            .line_glyph_count(cursor.page, cursor.line)
            .saturating_sub(1);
        self.set_text_cursor(cursor);
    }

    fn move_text_cursor_to_first_non_whitespace(&mut self) {
        self.count_buffer.clear();
        let mut cursor = self.current_text_cursor();
        cursor.glyph = self.first_non_whitespace_glyph(cursor.page, cursor.line);
        self.set_text_cursor(cursor);
    }

    fn set_text_cursor(&mut self, cursor: TextCursor) {
        let cursor = self.clamp_text_cursor(cursor);
        self.text_cursor = Some(cursor);
        self.focus_text_cursor(cursor);
        self.status = if matches!(self.mode, Mode::Visual | Mode::VisualLine) {
            self.visual_status()
        } else {
            self.default_status()
        };
    }

    fn current_text_cursor(&self) -> TextCursor {
        if matches!(self.mode, Mode::Visual | Mode::VisualLine) {
            return self
                .clamp_text_cursor(self.text_cursor.unwrap_or_else(|| self.cursor_at_view()));
        }

        self.clamp_text_cursor(self.text_cursor.unwrap_or_else(|| self.cursor_at_view()))
    }

    fn cursor_at_view(&self) -> TextCursor {
        let page = self.cursor_page();
        let line = self.cursor_line();
        let glyph = self.glyph_at_view_center(page, line);
        self.clamp_text_cursor(TextCursor { page, line, glyph })
    }

    fn glyph_at_view_center(&self, page: usize, line: usize) -> usize {
        let viewport = self.viewport();
        let layout = self.document_layout_for(viewport);
        let Some(page_layout) = layout.pages.get(page).copied() else {
            return 0;
        };
        let Some(page_ref) = self.document.pages.get(page) else {
            return 0;
        };
        let Some(line_ref) = page_ref.lines.get(line) else {
            return 0;
        };
        let viewport_center_x = self.viewport_offset.x.saturating_add(viewport.width / 2);
        let page_left = page_left_px(viewport.width, page_layout.bitmap_width);
        let target_x = viewport_center_x
            .saturating_sub(page_left)
            .min(page_layout.bitmap_width);

        line_ref
            .glyphs
            .iter()
            .enumerate()
            .min_by_key(|(_, glyph)| {
                project_pdf_center_x_to_page(glyph.bbox, page_ref.bbox, page_layout)
                    .abs_diff(target_x)
            })
            .map(|(glyph_index, _)| glyph_index)
            .unwrap_or(0)
    }

    fn clamp_text_cursor(&self, cursor: TextCursor) -> TextCursor {
        if self.document.pages.is_empty() {
            return TextCursor::default();
        }

        let page = cursor.page.min(self.document.pages.len() - 1);
        let page_ref = &self.document.pages[page];
        if page_ref.lines.is_empty() {
            return TextCursor {
                page,
                line: 0,
                glyph: 0,
            };
        }

        let line = cursor.line.min(page_ref.lines.len() - 1);
        let glyph_count = page_ref.lines[line].glyphs.len();
        let glyph = if glyph_count == 0 {
            0
        } else {
            cursor.glyph.min(glyph_count - 1)
        };

        TextCursor { page, line, glyph }
    }

    fn line_glyph_count(&self, page: usize, line: usize) -> usize {
        self.document
            .pages
            .get(page)
            .and_then(|page| page.lines.get(line))
            .map(|line| line.glyphs.len())
            .unwrap_or(0)
    }

    fn first_non_whitespace_glyph(&self, page: usize, line: usize) -> usize {
        self.document
            .pages
            .get(page)
            .and_then(|page| page.lines.get(line))
            .and_then(|line| {
                line.glyphs
                    .iter()
                    .position(|glyph| !glyph.ch.is_whitespace())
            })
            .unwrap_or(0)
    }

    fn next_word_start_cursor(&self, cursor: TextCursor) -> TextCursor {
        let mut cursor = self.clamp_text_cursor(cursor);

        if self
            .char_at_text_cursor(cursor)
            .map(is_word_char)
            .unwrap_or(false)
        {
            while let Some(next) = self.next_text_cursor(cursor) {
                if next.page != cursor.page || next.line != cursor.line {
                    cursor = next;
                    break;
                }
                cursor = next;
                if !self
                    .char_at_text_cursor(cursor)
                    .map(is_word_char)
                    .unwrap_or(false)
                {
                    break;
                }
            }
        }

        while !self
            .char_at_text_cursor(cursor)
            .map(is_word_char)
            .unwrap_or(false)
        {
            let Some(next) = self.next_text_cursor(cursor) else {
                return cursor;
            };
            cursor = next;
        }

        cursor
    }

    fn previous_word_start_cursor(&self, cursor: TextCursor) -> TextCursor {
        let mut cursor = self.clamp_text_cursor(cursor);
        while let Some(previous) = self.previous_text_cursor(cursor) {
            cursor = previous;
            if self
                .char_at_text_cursor(cursor)
                .map(is_word_char)
                .unwrap_or(false)
            {
                break;
            }
        }

        while let Some(previous) = self.previous_text_cursor(cursor) {
            if !self
                .char_at_text_cursor(previous)
                .map(is_word_char)
                .unwrap_or(false)
            {
                break;
            }
            cursor = previous;
        }

        cursor
    }

    fn next_text_cursor(&self, cursor: TextCursor) -> Option<TextCursor> {
        let cursor = self.clamp_text_cursor(cursor);
        let page = self.document.pages.get(cursor.page)?;
        let line = page.lines.get(cursor.line)?;
        if cursor.glyph + 1 < line.glyphs.len() {
            return Some(TextCursor {
                glyph: cursor.glyph + 1,
                ..cursor
            });
        }

        if cursor.line + 1 < page.lines.len() {
            return Some(self.clamp_text_cursor(TextCursor {
                line: cursor.line + 1,
                glyph: 0,
                ..cursor
            }));
        }

        if cursor.page + 1 < self.document.pages.len() {
            return Some(self.clamp_text_cursor(TextCursor {
                page: cursor.page + 1,
                line: 0,
                glyph: 0,
            }));
        }

        None
    }

    fn previous_text_cursor(&self, cursor: TextCursor) -> Option<TextCursor> {
        let cursor = self.clamp_text_cursor(cursor);
        if cursor.glyph > 0 {
            return Some(TextCursor {
                glyph: cursor.glyph - 1,
                ..cursor
            });
        }

        if cursor.line > 0 {
            let previous_line = cursor.line - 1;
            return Some(
                self.clamp_text_cursor(TextCursor {
                    line: previous_line,
                    glyph: self
                        .line_glyph_count(cursor.page, previous_line)
                        .saturating_sub(1),
                    ..cursor
                }),
            );
        }

        if cursor.page > 0 {
            let previous_page = cursor.page - 1;
            let previous_line = self
                .document
                .pages
                .get(previous_page)
                .map(|page| page.lines.len().saturating_sub(1))
                .unwrap_or(0);
            return Some(
                self.clamp_text_cursor(TextCursor {
                    page: previous_page,
                    line: previous_line,
                    glyph: self
                        .line_glyph_count(previous_page, previous_line)
                        .saturating_sub(1),
                }),
            );
        }

        None
    }

    fn char_at_text_cursor(&self, cursor: TextCursor) -> Option<char> {
        self.document
            .pages
            .get(cursor.page)
            .and_then(|page| page.lines.get(cursor.line))
            .and_then(|line| line.glyphs.get(cursor.glyph))
            .map(|glyph| glyph.ch)
    }

    fn next_line_cursor(&self, cursor: TextCursor) -> TextCursor {
        let cursor = self.clamp_text_cursor(cursor);
        let Some(page) = self.document.pages.get(cursor.page) else {
            return cursor;
        };

        if cursor.line + 1 < page.lines.len() {
            return self.clamp_text_cursor(TextCursor {
                line: cursor.line + 1,
                ..cursor
            });
        }

        let next_page = (cursor.page + 1).min(self.document.pages.len().saturating_sub(1));
        self.clamp_text_cursor(TextCursor {
            page: next_page,
            line: 0,
            glyph: cursor.glyph,
        })
    }

    fn previous_line_cursor(&self, cursor: TextCursor) -> TextCursor {
        let cursor = self.clamp_text_cursor(cursor);
        if cursor.line > 0 {
            return self.clamp_text_cursor(TextCursor {
                line: cursor.line - 1,
                ..cursor
            });
        }

        if cursor.page == 0 {
            return cursor;
        }

        let previous_page = cursor.page - 1;
        let previous_line = self
            .document
            .pages
            .get(previous_page)
            .map(|page| page.lines.len().saturating_sub(1))
            .unwrap_or(0);
        self.clamp_text_cursor(TextCursor {
            page: previous_page,
            line: previous_line,
            glyph: cursor.glyph,
        })
    }

    fn focus_text_cursor(&mut self, cursor: TextCursor) {
        let viewport = self.viewport();
        let layout = self.document_layout_for(viewport);
        let Some(page_layout) = layout.pages.get(cursor.page).copied() else {
            return;
        };
        let page = &self.document.pages[cursor.page];
        let line_bbox = page.lines.get(cursor.line).map(|line| line.bbox);
        let focus_bbox = page
            .lines
            .get(cursor.line)
            .and_then(|line| line.glyphs.get(cursor.glyph).or_else(|| line.glyphs.last()))
            .map(|glyph| glyph.bbox)
            .or(line_bbox)
            .unwrap_or(page.bbox);
        let focus_x = project_pdf_center_x_to_page(focus_bbox, page.bbox, page_layout);
        let focus_y = project_pdf_center_y_to_page(focus_bbox, page.bbox, page_layout);
        let page_left = page_left_px(viewport.width, page_layout.bitmap_width);

        self.viewport_offset.x = page_left
            .saturating_add(focus_x)
            .saturating_sub(viewport.width / 2);
        self.viewport_offset.y = page_layout
            .doc_y
            .saturating_add(focus_y)
            .saturating_sub(viewport.height / 2);
        self.clamp_viewport_offset_with(&layout, viewport);
        self.bump_render_nonce();
    }

    fn visual_status(&self) -> String {
        let cursor = self.current_text_cursor();
        match self.mode {
            Mode::Visual => format!(
                "visual: p{} line {} char {} | y copy | Esc exit",
                cursor.page + 1,
                cursor.line + 1,
                cursor.glyph + 1
            ),
            Mode::VisualLine => format!(
                "visual line: p{} line {} | y copy | Esc exit",
                cursor.page + 1,
                cursor.line + 1
            ),
            _ => self.default_status(),
        }
    }

    fn visual_selection_ranges(&self) -> Vec<TextSelectionRange> {
        let Some(anchor) = self.visual_anchor else {
            return Vec::new();
        };
        if !matches!(self.mode, Mode::Visual | Mode::VisualLine) {
            return Vec::new();
        }

        let start = self
            .clamp_text_cursor(anchor)
            .min(self.current_text_cursor());
        let end = self
            .clamp_text_cursor(anchor)
            .max(self.current_text_cursor());
        let mut ranges = Vec::new();

        for page_index in start.page..=end.page {
            let Some(page) = self.document.pages.get(page_index) else {
                continue;
            };
            if page.lines.is_empty() {
                continue;
            }

            let first_line = if page_index == start.page {
                start.line
            } else {
                0
            };
            let last_line = if page_index == end.page {
                end.line.min(page.lines.len() - 1)
            } else {
                page.lines.len() - 1
            };

            for line_index in first_line..=last_line {
                let glyph_count = page.lines[line_index].glyphs.len();
                if glyph_count == 0 {
                    ranges.push(TextSelectionRange {
                        page: page_index,
                        line: line_index,
                        start_glyph: 0,
                        end_glyph: 0,
                    });
                    continue;
                }

                let (start_glyph, end_glyph) = if self.mode == Mode::VisualLine {
                    (0, glyph_count - 1)
                } else if start.page == end.page && start.line == end.line {
                    (
                        start.glyph.min(glyph_count - 1),
                        end.glyph.min(glyph_count - 1),
                    )
                } else if page_index == start.page && line_index == start.line {
                    (start.glyph.min(glyph_count - 1), glyph_count - 1)
                } else if page_index == end.page && line_index == end.line {
                    (0, end.glyph.min(glyph_count - 1))
                } else {
                    (0, glyph_count - 1)
                };

                ranges.push(TextSelectionRange {
                    page: page_index,
                    line: line_index,
                    start_glyph,
                    end_glyph,
                });
            }
        }

        ranges
    }

    fn visual_selected_text(&self) -> Option<String> {
        let ranges = self.visual_selection_ranges();
        if ranges.is_empty() {
            return None;
        }

        Some(
            ranges
                .into_iter()
                .map(|range| self.text_for_selection_range(range))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }

    fn text_for_selection_range(&self, range: TextSelectionRange) -> String {
        let Some(line) = self
            .document
            .pages
            .get(range.page)
            .and_then(|page| page.lines.get(range.line))
        else {
            return String::new();
        };
        if line.glyphs.is_empty() {
            return String::new();
        }

        let start = range.start_glyph.min(line.glyphs.len() - 1);
        let end = range.end_glyph.min(line.glyphs.len() - 1);
        line.glyphs[start.min(end)..=start.max(end)]
            .iter()
            .map(|glyph| glyph.ch)
            .collect()
    }

    fn selection_range_bounds(&self, range: TextSelectionRange) -> Option<PdfRect> {
        let line = self.document.pages.get(range.page)?.lines.get(range.line)?;
        if line.glyphs.is_empty() {
            return Some(line.bbox);
        }

        let start = range.start_glyph.min(line.glyphs.len() - 1);
        let end = range.end_glyph.min(line.glyphs.len() - 1);
        let mut glyphs = line.glyphs[start.min(end)..=start.max(end)].iter();
        let first = glyphs.next()?;
        let mut min_x = first.bbox.x;
        let mut min_y = first.bbox.y;
        let mut max_x = first.bbox.x + first.bbox.width;
        let mut max_y = first.bbox.y + first.bbox.height;

        for glyph in glyphs {
            min_x = min_x.min(glyph.bbox.x);
            min_y = min_y.min(glyph.bbox.y);
            max_x = max_x.max(glyph.bbox.x + glyph.bbox.width);
            max_y = max_y.max(glyph.bbox.y + glyph.bbox.height);
        }

        Some(PdfRect::new(min_x, min_y, max_x - min_x, max_y - min_y))
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
        self.visual_anchor = None;
        self.text_cursor = None;
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
        self.text_cursor = Some(self.clamp_text_cursor(TextCursor {
            page: page_index,
            line: line_index,
            glyph: 0,
        }));
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
        self.text_cursor = None;
        self.status = self.default_status();
    }

    fn adjust_zoom_at_mouse(&mut self, mouse: MouseEvent, delta_percent: i16) {
        let viewport = self.viewport();
        let old_layout = self.document_layout_for(viewport);
        let mouse_x = mouse
            .column
            .saturating_sub(viewport.area.x)
            .min(viewport.area.width);
        let mouse_y = mouse
            .row
            .saturating_sub(viewport.area.y)
            .min(viewport.area.height);
        let anchor_x = self
            .viewport_offset
            .x
            .saturating_add(u32::from(mouse_x) * u32::from(viewport.cell.width));
        let anchor_y = self
            .viewport_offset
            .y
            .saturating_add(u32::from(mouse_y) * u32::from(viewport.cell.height));
        let current_page = current_page_for_scroll(&old_layout, anchor_y, 1)
            .min(self.document.page_count().saturating_sub(1));
        let old_page_layout = old_layout.pages.get(current_page).copied();

        let next = (self.zoom_percent as i16 + delta_percent).clamp(25, 400);
        self.zoom_percent = next as u16;

        let new_layout = self.document_layout_for(viewport);
        if let (Some(old_page_layout), Some(new_page_layout)) =
            (old_page_layout, new_layout.pages.get(current_page).copied())
        {
            let old_page_left = page_left_px(viewport.width, old_page_layout.bitmap_width);
            let new_page_left = page_left_px(viewport.width, new_page_layout.bitmap_width);
            let relative_x =
                relative_position(anchor_x, old_page_left, old_page_layout.bitmap_width);
            let relative_y = relative_position(
                anchor_y,
                old_page_layout.doc_y,
                old_page_layout.bitmap_height,
            );
            let new_anchor_x = new_page_left
                .saturating_add((relative_x * new_page_layout.bitmap_width as f32).round() as u32);
            let new_anchor_y = new_page_layout
                .doc_y
                .saturating_add((relative_y * new_page_layout.bitmap_height as f32).round() as u32);

            self.viewport_offset.x =
                new_anchor_x.saturating_sub(u32::from(mouse_x) * u32::from(viewport.cell.width));
            self.viewport_offset.y =
                new_anchor_y.saturating_sub(u32::from(mouse_y) * u32::from(viewport.cell.height));
        }

        self.clamp_viewport_offset_with(&new_layout, viewport);
        self.bump_render_nonce();
        self.text_cursor = None;
        self.status = self.default_status();
    }

    fn reset_zoom(&mut self) {
        self.zoom_percent = 100;
        self.viewport_offset = ViewportOffset::default();
        self.text_cursor = None;
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
        self.text_cursor = None;
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
        self.text_cursor = None;
        self.count_buffer.clear();
        self.bump_render_nonce();
        self.status = self.default_status();
    }

    fn mouse_vertical_scroll_step(&self) -> u32 {
        let viewport = self.viewport();
        u32::from(viewport.cell.height) * MOUSE_SCROLL_ROWS
    }

    fn mouse_horizontal_scroll_step(&self) -> u32 {
        let viewport = self.viewport();
        u32::from(viewport.cell.width) * MOUSE_SCROLL_COLUMNS
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

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn coalesce_axis_i32(current: i32, delta: i32) -> i32 {
    if current == 0 || current.signum() == delta.signum() {
        delta
    } else {
        0
    }
}

fn coalesce_axis_i16(current: i16, delta: i16) -> i16 {
    if current == 0 || current.signum() == delta.signum() {
        delta
    } else {
        0
    }
}

fn coalesce_axis_isize(current: isize, delta: isize) -> isize {
    if current == 0 || current.signum() == delta.signum() {
        delta
    } else {
        0
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

fn copy_to_system_clipboard(text: &str) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        write_text_to_command("pbcopy", &[], text)
    }

    #[cfg(target_os = "windows")]
    {
        return write_text_to_command("clip", &[], text);
    }

    #[cfg(target_os = "linux")]
    {
        for (program, args) in [
            ("wl-copy", Vec::<&str>::new()),
            ("xclip", vec!["-selection", "clipboard"]),
            ("xsel", vec!["--clipboard", "--input"]),
        ] {
            if write_text_to_command(program, &args, text).is_ok() {
                return Ok(());
            }
        }
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no supported clipboard command found (tried wl-copy, xclip, xsel)",
        ));
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = text;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "clipboard copy is not supported on this platform",
        ))
    }
}

fn write_text_to_command(program: &str, args: &[&str], text: &str) -> io::Result<()> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .spawn()?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(text.as_bytes())?;
    }

    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "clipboard command {program} exited with {status}"
        )))
    }
}
