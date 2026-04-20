use std::collections::HashMap;

use crossterm::terminal::WindowSize;
use ratatui::layout::Rect;

use crate::document::{Document, PdfRect};

const FALLBACK_CELL_WIDTH_PX: u16 = 8;
const FALLBACK_CELL_HEIGHT_PX: u16 = 16;
const DEFAULT_RENDER_CACHE_CAPACITY: usize = 32;
pub const PAGE_GAP_PX: u32 = 24;
pub const PAGE_PRELOAD_RADIUS: usize = 5;
pub const SEARCH_HIGHLIGHT_RGBA: [u8; 4] = [255, 235, 59, 96];
pub const ACTIVE_SEARCH_HIGHLIGHT_RGBA: [u8; 4] = [255, 193, 7, 160];
pub const FOLLOW_TAG_BG_RGBA: [u8; 4] = [255, 235, 59, 255];
pub const FOLLOW_TAG_FG_RGBA: [u8; 4] = [0, 0, 0, 255];
const FOLLOW_TAG_FONT_SCALE: u32 = 2;
const FOLLOW_TAG_GLYPH_WIDTH: u32 = 5;
const FOLLOW_TAG_GLYPH_HEIGHT: u32 = 7;
const FOLLOW_TAG_GLYPH_SPACING: u32 = 1;
const FOLLOW_TAG_PADDING_X: u32 = 4;
const FOLLOW_TAG_PADDING_Y: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellPixels {
    pub width: u16,
    pub height: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewportPixels {
    pub area: Rect,
    pub cell: CellPixels,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewportOffset {
    pub x: u32,
    pub y: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentLayoutPage {
    pub page_index: usize,
    pub doc_y: u32,
    pub bitmap_width: u32,
    pub bitmap_height: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentLayout {
    pub pages: Vec<DocumentLayoutPage>,
    pub total_height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisiblePagePlan {
    pub page_index: usize,
    pub area: Rect,
    pub placement_col: u16,
    pub placement_row: u16,
    pub bitmap_width: u32,
    pub bitmap_height: u32,
    pub crop_x: u32,
    pub crop_y: u32,
    pub crop_width: u32,
    pub crop_height: u32,
    pub placement_columns: u16,
    pub placement_rows: u16,
    pub frame_offset_x: u16,
    pub frame_offset_y: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameOffsets {
    pub x: u32,
    pub y: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FollowTag {
    pub bounds: PdfRect,
    pub label: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageRenderPlan {
    pub page_index: usize,
    pub area: Rect,
    pub placement_col: u16,
    pub placement_row: u16,
    pub bitmap_width: u32,
    pub bitmap_height: u32,
    pub crop_x: u32,
    pub crop_y: u32,
    pub crop_width: u32,
    pub crop_height: u32,
    pub placement_columns: u16,
    pub placement_rows: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PageRenderInfo {
    pub page_index: usize,
    pub placement_col: u16,
    pub placement_row: u16,
    pub bitmap_width: u32,
    pub bitmap_height: u32,
    pub crop_x: u32,
    pub crop_y: u32,
    pub crop_width: u32,
    pub crop_height: u32,
    pub placement_columns: u16,
    pub placement_rows: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedPage {
    pub page_index: usize,
    pub placement_col: u16,
    pub placement_row: u16,
    pub bitmap_width: u32,
    pub bitmap_height: u32,
    pub crop_x: u32,
    pub crop_y: u32,
    pub crop_width: u32,
    pub crop_height: u32,
    pub placement_columns: u16,
    pub placement_rows: u16,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverlayPlacement {
    pub cell_x: u16,
    pub cell_y: u16,
    pub columns: u16,
    pub rows: u16,
    pub offset_x: u16,
    pub offset_y: u16,
    pub width_px: u16,
    pub height_px: u16,
    pub cell: CellPixels,
}

impl RenderedPage {
    pub fn info(&self) -> PageRenderInfo {
        PageRenderInfo {
            page_index: self.page_index,
            placement_col: self.placement_col,
            placement_row: self.placement_row,
            bitmap_width: self.bitmap_width,
            bitmap_height: self.bitmap_height,
            crop_x: self.crop_x,
            crop_y: self.crop_y,
            crop_width: self.crop_width,
            crop_height: self.crop_height,
            placement_columns: self.placement_columns,
            placement_rows: self.placement_rows,
        }
    }
}

pub fn build_highlight_mask(
    page_index: usize,
    placement: OverlayPlacement,
    rgba: [u8; 4],
) -> Option<RenderedPage> {
    if placement.columns == 0 || placement.rows == 0 {
        return None;
    }

    let bitmap_width = u32::from(placement.columns) * u32::from(placement.cell.width);
    let bitmap_height = u32::from(placement.rows) * u32::from(placement.cell.height);
    let mut bytes = vec![0u8; (bitmap_width * bitmap_height * 4) as usize];

    let start_x = u32::from(placement.offset_x);
    let start_y = u32::from(placement.offset_y);
    let end_x = (start_x + u32::from(placement.width_px)).min(bitmap_width);
    let end_y = (start_y + u32::from(placement.height_px)).min(bitmap_height);

    for y in start_y..end_y {
        for x in start_x..end_x {
            let index = ((y * bitmap_width + x) * 4) as usize;
            bytes[index..index + 4].copy_from_slice(&rgba);
        }
    }

    Some(RenderedPage {
        page_index,
        placement_col: placement.cell_x,
        placement_row: placement.cell_y,
        bitmap_width,
        bitmap_height,
        crop_x: 0,
        crop_y: 0,
        crop_width: bitmap_width,
        crop_height: bitmap_height,
        placement_columns: placement.columns,
        placement_rows: placement.rows,
        rgba: bytes,
    })
}

pub fn compose_visible_page_frame(
    source: &RenderedPage,
    page_bbox: PdfRect,
    cell: CellPixels,
    dark_mode: bool,
    passive_highlights: &[PdfRect],
    active_highlight: Option<PdfRect>,
    follow_tags: &[FollowTag],
) -> RenderedPage {
    compose_visible_page_frame_with_offsets(
        source,
        page_bbox,
        cell,
        dark_mode,
        passive_highlights,
        active_highlight,
        follow_tags,
        None,
    )
}

pub fn compose_visible_page_frame_with_offsets(
    source: &RenderedPage,
    page_bbox: PdfRect,
    cell: CellPixels,
    dark_mode: bool,
    passive_highlights: &[PdfRect],
    active_highlight: Option<PdfRect>,
    follow_tags: &[FollowTag],
    frame_offsets: Option<FrameOffsets>,
) -> RenderedPage {
    let frame_width = u32::from(source.placement_columns.max(1)) * u32::from(cell.width.max(1));
    let frame_height = u32::from(source.placement_rows.max(1)) * u32::from(cell.height.max(1));
    let mut rgba = vec![0u8; (frame_width * frame_height * 4) as usize];

    let copy_width = source
        .crop_width
        .min(source.bitmap_width.saturating_sub(source.crop_x))
        .min(frame_width);
    let copy_height = source
        .crop_height
        .min(source.bitmap_height.saturating_sub(source.crop_y))
        .min(frame_height);
    let default_offsets = FrameOffsets {
        x: frame_width.saturating_sub(copy_width) / 2,
        y: frame_height.saturating_sub(copy_height) / 2,
    };
    let frame_offsets = frame_offsets.unwrap_or(default_offsets);
    let frame_offset_x = frame_offsets.x.min(frame_width.saturating_sub(copy_width));
    let frame_offset_y = frame_offsets
        .y
        .min(frame_height.saturating_sub(copy_height));

    for y in 0..copy_height {
        let src_offset = (((source.crop_y + y) * source.bitmap_width + source.crop_x) * 4) as usize;
        let dst_offset = (((frame_offset_y + y) * frame_width + frame_offset_x) * 4) as usize;
        let row_bytes = (copy_width * 4) as usize;
        rgba[dst_offset..dst_offset + row_bytes]
            .copy_from_slice(&source.rgba[src_offset..src_offset + row_bytes]);
    }

    if dark_mode {
        invert_visible_region_with_offset(
            &mut rgba,
            frame_width,
            copy_width,
            copy_height,
            frame_offset_x,
            frame_offset_y,
        );
    }

    for bounds in passive_highlights {
        blend_pdf_rect_highlight(
            &mut rgba,
            frame_width,
            copy_width,
            copy_height,
            frame_offset_x,
            frame_offset_y,
            source,
            page_bbox,
            *bounds,
            SEARCH_HIGHLIGHT_RGBA,
        );
    }

    if let Some(bounds) = active_highlight {
        blend_pdf_rect_highlight(
            &mut rgba,
            frame_width,
            copy_width,
            copy_height,
            frame_offset_x,
            frame_offset_y,
            source,
            page_bbox,
            bounds,
            ACTIVE_SEARCH_HIGHLIGHT_RGBA,
        );
    }

    for tag in follow_tags {
        blend_follow_tag(
            &mut rgba,
            frame_width,
            copy_width,
            copy_height,
            frame_offset_x,
            frame_offset_y,
            source,
            page_bbox,
            tag,
        );
    }

    RenderedPage {
        page_index: source.page_index,
        placement_col: source.placement_col,
        placement_row: source.placement_row,
        bitmap_width: frame_width,
        bitmap_height: frame_height,
        crop_x: 0,
        crop_y: 0,
        crop_width: frame_width,
        crop_height: frame_height,
        placement_columns: source.placement_columns,
        placement_rows: source.placement_rows,
        rgba,
    }
}

pub fn invert_rgba_in_place(bytes: &mut [u8]) {
    for chunk in bytes.chunks_exact_mut(4) {
        chunk[0] = 255 - chunk[0];
        chunk[1] = 255 - chunk[1];
        chunk[2] = 255 - chunk[2];
    }
}

pub struct PageRenderCache {
    entries: HashMap<PageRenderInfo, RenderedPage>,
    order: Vec<PageRenderInfo>,
    capacity: usize,
}

impl Default for PageRenderCache {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            order: Vec::new(),
            capacity: DEFAULT_RENDER_CACHE_CAPACITY,
        }
    }
}

impl PageRenderCache {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: Vec::new(),
            capacity: capacity.max(1),
        }
    }

    fn touch(&mut self, key: PageRenderInfo) {
        self.order.retain(|existing| existing != &key);
        self.order.push(key);
    }

    fn evict_if_needed(&mut self) {
        while self.entries.len() > self.capacity {
            if let Some(oldest) = self.order.first().copied() {
                self.order.remove(0);
                self.entries.remove(&oldest);
            } else {
                break;
            }
        }
    }

    #[allow(dead_code)]
    pub fn contains(&self, key: PageRenderInfo) -> bool {
        self.entries.contains_key(&key)
    }

    #[allow(dead_code)]
    pub fn get(&self, key: PageRenderInfo) -> Option<&RenderedPage> {
        self.entries.get(&key)
    }

    #[allow(dead_code)]
    pub fn insert(&mut self, rendered: RenderedPage) -> &RenderedPage {
        let key = rendered.info();
        self.entries.insert(key, rendered);
        self.touch(key);
        self.evict_if_needed();
        self.entries.get(&key).expect("render cache just inserted")
    }

    pub fn get_or_insert_with<E, F>(
        &mut self,
        key: PageRenderInfo,
        render: F,
    ) -> Result<&RenderedPage, E>
    where
        F: FnOnce() -> Result<RenderedPage, E>,
    {
        if self.entries.contains_key(&key) {
            self.touch(key);
            return Ok(self.entries.get(&key).expect("render cache key exists"));
        }

        let rendered = render()?;
        self.entries.insert(key, rendered);
        self.touch(key);
        self.evict_if_needed();

        Ok(self.entries.get(&key).expect("render cache just inserted"))
    }
}

pub fn viewport_pixels(area: Rect, window: WindowSize) -> ViewportPixels {
    let cell = estimate_cell_pixels(window);

    ViewportPixels {
        area,
        cell,
        width: u32::from(area.width) * u32::from(cell.width),
        height: u32::from(area.height) * u32::from(cell.height),
    }
}

pub fn build_page_render_plan(
    page_index: usize,
    page_bbox: PdfRect,
    viewport: ViewportPixels,
    zoom_factor: f32,
    viewport_offset: ViewportOffset,
) -> Option<PageRenderPlan> {
    if viewport.area.width == 0 || viewport.area.height == 0 {
        return None;
    }

    let (fit_width, fit_height) = fit_page_to_pixels_by_height(page_bbox, viewport.height);
    let zoom = zoom_factor.max(0.1);
    let bitmap_width = (fit_width as f32 * zoom).round().max(1.0) as u32;
    let bitmap_height = (fit_height as f32 * zoom).round().max(1.0) as u32;
    let crop_width = viewport.width;
    let crop_height = viewport.height;

    if bitmap_width == 0 || bitmap_height == 0 {
        return None;
    }

    let placement_columns = viewport.area.width;
    let placement_rows = viewport.area.height;
    let horizontal_padding = viewport.area.width.saturating_sub(placement_columns) / 2;
    let vertical_padding = viewport.area.height.saturating_sub(placement_rows) / 2;

    let max_crop_x = bitmap_width.saturating_sub(crop_width);
    let max_crop_y = bitmap_height.saturating_sub(crop_height);

    Some(PageRenderPlan {
        page_index,
        area: viewport.area,
        placement_col: viewport.area.x + horizontal_padding,
        placement_row: viewport.area.y + vertical_padding,
        bitmap_width,
        bitmap_height,
        crop_x: viewport_offset.x.min(max_crop_x),
        crop_y: viewport_offset.y.min(max_crop_y),
        crop_width,
        crop_height,
        placement_columns,
        placement_rows,
    })
}

pub fn build_document_layout(
    document: &Document,
    viewport: ViewportPixels,
    zoom_factor: f32,
) -> DocumentLayout {
    let mut pages = Vec::with_capacity(document.pages.len());
    let mut doc_y = 0u32;

    for (page_index, page) in document.pages.iter().enumerate() {
        let (bitmap_width, bitmap_height) =
            fit_page_to_pixels_by_height(page.bbox, viewport.height);
        let zoom = zoom_factor.max(0.1);
        let bitmap_width = (bitmap_width as f32 * zoom).round().max(1.0) as u32;
        let bitmap_height = (bitmap_height as f32 * zoom).round().max(1.0) as u32;

        pages.push(DocumentLayoutPage {
            page_index,
            doc_y,
            bitmap_width,
            bitmap_height,
        });

        doc_y = doc_y
            .saturating_add(bitmap_height)
            .saturating_add(PAGE_GAP_PX);
    }

    let total_height = pages
        .last()
        .map(|page| page.doc_y.saturating_add(page.bitmap_height))
        .unwrap_or(0);

    DocumentLayout {
        pages,
        total_height,
    }
}

pub fn current_page_for_scroll(
    layout: &DocumentLayout,
    scroll_y: u32,
    viewport_height: u32,
) -> usize {
    let center_y = scroll_y.saturating_add(viewport_height / 2);

    layout
        .pages
        .iter()
        .min_by_key(|page| distance_to_vertical_range(center_y, page.doc_y, page.bitmap_height))
        .map(|page| page.page_index)
        .unwrap_or(0)
}

pub fn build_visible_page_plans(
    layout: &DocumentLayout,
    viewport: ViewportPixels,
    viewport_offset: ViewportOffset,
) -> Vec<VisiblePagePlan> {
    if viewport.area.width == 0 || viewport.area.height == 0 {
        return Vec::new();
    }

    let current_page = current_page_for_scroll(layout, viewport_offset.y, viewport.height);
    let current_page = current_page.min(layout.pages.len().saturating_sub(1));
    let preload_start = current_page.saturating_sub(PAGE_PRELOAD_RADIUS);
    let preload_end = current_page
        .saturating_add(PAGE_PRELOAD_RADIUS)
        .min(layout.pages.len().saturating_sub(1));
    let viewport_top = viewport_offset.y;
    let viewport_bottom = viewport_offset.y.saturating_add(viewport.height);

    layout
        .pages
        .iter()
        .filter(|page| {
            let page_bottom = page.doc_y.saturating_add(page.bitmap_height);
            let intersects_viewport = page_bottom > viewport_top && page.doc_y < viewport_bottom;
            let inside_preload_window =
                page.page_index >= preload_start && page.page_index <= preload_end;
            intersects_viewport || inside_preload_window
        })
        .filter_map(|page| build_visible_page_plan(*page, viewport, viewport_offset))
        .collect()
}

fn build_visible_page_plan(
    page: DocumentLayoutPage,
    viewport: ViewportPixels,
    viewport_offset: ViewportOffset,
) -> Option<VisiblePagePlan> {
    if page.bitmap_width == 0 || page.bitmap_height == 0 {
        return None;
    }

    let visible_top = viewport_offset.y.max(page.doc_y);
    let visible_bottom = viewport_offset
        .y
        .saturating_add(viewport.height)
        .min(page.doc_y.saturating_add(page.bitmap_height));

    if visible_top >= visible_bottom {
        return None;
    }

    let (crop_x, crop_width, placement_x_px) = if page.bitmap_width <= viewport.width {
        (
            0,
            page.bitmap_width,
            viewport.width.saturating_sub(page.bitmap_width) / 2,
        )
    } else {
        let crop_x = viewport_offset
            .x
            .min(page.bitmap_width.saturating_sub(viewport.width));
        (crop_x, viewport.width, 0)
    };
    let crop_y = visible_top.saturating_sub(page.doc_y);
    let crop_height = visible_bottom.saturating_sub(visible_top);
    let placement_y_px = visible_top.saturating_sub(viewport_offset.y);
    let frame_offset_x = (placement_x_px % u32::from(viewport.cell.width.max(1))) as u16;
    let frame_offset_y = (placement_y_px % u32::from(viewport.cell.height.max(1))) as u16;
    let placement_col =
        viewport.area.x + (placement_x_px / u32::from(viewport.cell.width.max(1))) as u16;
    let placement_row =
        viewport.area.y + (placement_y_px / u32::from(viewport.cell.height.max(1))) as u16;
    let placement_columns = (u32::from(frame_offset_x) + crop_width)
        .div_ceil(u32::from(viewport.cell.width.max(1))) as u16;
    let placement_rows = (u32::from(frame_offset_y) + crop_height)
        .div_ceil(u32::from(viewport.cell.height.max(1))) as u16;

    Some(VisiblePagePlan {
        page_index: page.page_index,
        area: viewport.area,
        placement_col,
        placement_row,
        bitmap_width: page.bitmap_width,
        bitmap_height: page.bitmap_height,
        crop_x,
        crop_y,
        crop_width,
        crop_height,
        placement_columns: placement_columns.max(1),
        placement_rows: placement_rows.max(1),
        frame_offset_x,
        frame_offset_y,
    })
}

fn distance_to_vertical_range(position: u32, start: u32, extent: u32) -> u32 {
    let end = start.saturating_add(extent);
    if position < start {
        start.saturating_sub(position)
    } else if position >= end {
        position.saturating_sub(end)
    } else {
        0
    }
}

impl PageRenderPlan {
    pub fn info(&self) -> PageRenderInfo {
        PageRenderInfo {
            page_index: self.page_index,
            placement_col: self.placement_col,
            placement_row: self.placement_row,
            bitmap_width: self.bitmap_width,
            bitmap_height: self.bitmap_height,
            crop_x: self.crop_x,
            crop_y: self.crop_y,
            crop_width: self.crop_width,
            crop_height: self.crop_height,
            placement_columns: self.placement_columns,
            placement_rows: self.placement_rows,
        }
    }
}

fn estimate_cell_pixels(window: WindowSize) -> CellPixels {
    if window.columns > 0 && window.rows > 0 && window.width > 0 && window.height > 0 {
        return CellPixels {
            width: (window.width / window.columns).max(1),
            height: (window.height / window.rows).max(1),
        };
    }

    CellPixels {
        width: FALLBACK_CELL_WIDTH_PX,
        height: FALLBACK_CELL_HEIGHT_PX,
    }
}

fn fit_page_to_pixels_by_height(page_bbox: PdfRect, target_height: u32) -> (u32, u32) {
    if target_height == 0 {
        return (0, 0);
    }

    let page_width = page_bbox.width.max(1.0);
    let page_height = page_bbox.height.max(1.0);
    let scale = target_height as f32 / page_height;

    let width = (page_width * scale).round().max(1.0) as u32;
    let height = (page_height * scale).round().max(1.0) as u32;

    (width, height)
}

fn invert_visible_region_with_offset(
    bytes: &mut [u8],
    stride_width: u32,
    visible_width: u32,
    visible_height: u32,
    offset_x: u32,
    offset_y: u32,
) {
    for y in 0..visible_height {
        let row_offset = (((offset_y + y) * stride_width + offset_x) * 4) as usize;
        let row_end = row_offset + (visible_width * 4) as usize;
        invert_rgba_in_place(&mut bytes[row_offset..row_end]);
    }
}

fn blend_pdf_rect_highlight(
    rgba: &mut [u8],
    frame_width: u32,
    visible_width: u32,
    visible_height: u32,
    frame_offset_x: u32,
    frame_offset_y: u32,
    source: &RenderedPage,
    page_bbox: PdfRect,
    bounds: PdfRect,
    color: [u8; 4],
) {
    let Some((left, top, right, bottom)) = project_pdf_rect_to_visible_pixels(
        source,
        page_bbox,
        bounds,
        visible_width,
        visible_height,
    ) else {
        return;
    };

    for y in top..bottom {
        for x in left..right {
            let offset = ((((frame_offset_y + y) * frame_width) + frame_offset_x + x) * 4) as usize;
            blend_rgba_over(&mut rgba[offset..offset + 4], color);
        }
    }
}

fn blend_follow_tag(
    rgba: &mut [u8],
    frame_width: u32,
    visible_width: u32,
    visible_height: u32,
    frame_offset_x: u32,
    frame_offset_y: u32,
    source: &RenderedPage,
    page_bbox: PdfRect,
    tag: &FollowTag,
) {
    let Some((left, top, _, _)) = project_pdf_rect_to_visible_pixels(
        source,
        page_bbox,
        tag.bounds,
        visible_width,
        visible_height,
    ) else {
        return;
    };

    let (tag_width, tag_height) = follow_tag_badge_size(&tag.label);
    let tag_width = tag_width.min(visible_width.saturating_sub(left));
    let tag_height = tag_height.min(visible_height.saturating_sub(top));
    let glyph_advance = (FOLLOW_TAG_GLYPH_WIDTH + FOLLOW_TAG_GLYPH_SPACING) * FOLLOW_TAG_FONT_SCALE;
    let bg_right = left.saturating_add(tag_width).min(visible_width);
    let bg_bottom = top.saturating_add(tag_height).min(visible_height);

    for y in top..bg_bottom {
        for x in left..bg_right {
            let offset = ((((frame_offset_y + y) * frame_width) + frame_offset_x + x) * 4) as usize;
            blend_rgba_over(&mut rgba[offset..offset + 4], FOLLOW_TAG_BG_RGBA);
        }
    }

    let mut cursor_x = left + FOLLOW_TAG_PADDING_X;
    let baseline_y = top + FOLLOW_TAG_PADDING_Y;
    for ch in tag.label.chars() {
        draw_glyph_5x7_scaled(
            rgba,
            frame_width,
            frame_offset_x,
            frame_offset_y,
            cursor_x,
            baseline_y,
            ch,
            FOLLOW_TAG_FG_RGBA,
            bg_right,
            bg_bottom,
        );
        cursor_x = cursor_x.saturating_add(glyph_advance);
    }
}

pub fn follow_tag_badge_size(label: &str) -> (u32, u32) {
    let glyph_advance = (FOLLOW_TAG_GLYPH_WIDTH + FOLLOW_TAG_GLYPH_SPACING) * FOLLOW_TAG_FONT_SCALE;
    let width = (label.len() as u32)
        .saturating_mul(glyph_advance)
        .saturating_add(FOLLOW_TAG_PADDING_X * 2);
    let height = FOLLOW_TAG_GLYPH_HEIGHT * FOLLOW_TAG_FONT_SCALE + FOLLOW_TAG_PADDING_Y * 2;
    (width.max(1), height.max(1))
}

fn draw_glyph_5x7_scaled(
    rgba: &mut [u8],
    frame_width: u32,
    frame_offset_x: u32,
    frame_offset_y: u32,
    origin_x: u32,
    origin_y: u32,
    ch: char,
    color: [u8; 4],
    clip_right: u32,
    clip_bottom: u32,
) {
    let glyph = glyph_5x7(ch);
    for (row, pattern) in glyph.iter().enumerate() {
        for col in 0..FOLLOW_TAG_GLYPH_WIDTH {
            if (pattern >> (4 - col)) & 1 == 0 {
                continue;
            }

            for dy in 0..FOLLOW_TAG_FONT_SCALE {
                for dx in 0..FOLLOW_TAG_FONT_SCALE {
                    let x = origin_x
                        .saturating_add(col * FOLLOW_TAG_FONT_SCALE)
                        .saturating_add(dx);
                    let y = origin_y
                        .saturating_add(row as u32 * FOLLOW_TAG_FONT_SCALE)
                        .saturating_add(dy);
                    if x >= clip_right || y >= clip_bottom {
                        continue;
                    }

                    let offset =
                        ((((frame_offset_y + y) * frame_width) + frame_offset_x + x) * 4) as usize;
                    blend_rgba_over(&mut rgba[offset..offset + 4], color);
                }
            }
        }
    }
}

fn glyph_5x7(ch: char) -> [u8; 7] {
    match ch.to_ascii_lowercase() {
        'a' => [
            0b01110, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001, 0b10001,
        ],
        'b' => [
            0b11110, 0b10001, 0b11110, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'c' => [
            0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111,
        ],
        'd' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'e' => [
            0b11111, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'f' => [
            0b11111, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000, 0b10000,
        ],
        'g' => [
            0b01111, 0b10000, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111,
        ],
        'h' => [
            0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001, 0b10001,
        ],
        'i' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
        'j' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100,
        ],
        'k' => [
            0b10001, 0b10010, 0b11100, 0b11000, 0b11100, 0b10010, 0b10001,
        ],
        'l' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'm' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'n' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'o' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'p' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'r' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        's' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        't' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'u' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'v' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'w' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001,
        ],
        'x' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
        '6' => [
            0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
        ],
        _ => [
            0b11111, 0b10001, 0b00100, 0b00100, 0b00100, 0b10001, 0b11111,
        ],
    }
}

fn project_pdf_rect_to_visible_pixels(
    source: &RenderedPage,
    page_bbox: PdfRect,
    bounds: PdfRect,
    visible_width: u32,
    visible_height: u32,
) -> Option<(u32, u32, u32, u32)> {
    if page_bbox.width <= 0.0
        || page_bbox.height <= 0.0
        || visible_width == 0
        || visible_height == 0
    {
        return None;
    }

    let scale_x = source.bitmap_width as f32 / page_bbox.width;
    let scale_y = source.bitmap_height as f32 / page_bbox.height;
    let projected_top = (page_bbox.height - (bounds.y + bounds.height)).max(0.0);
    let projected_bottom = (page_bbox.height - bounds.y).max(0.0);

    let left = (bounds.x * scale_x).floor() as i32 - source.crop_x as i32;
    let top = (projected_top * scale_y).floor() as i32 - source.crop_y as i32;
    let right = ((bounds.x + bounds.width) * scale_x).ceil() as i32 - source.crop_x as i32;
    let bottom = (projected_bottom * scale_y).ceil() as i32 - source.crop_y as i32;

    let clamped_left = left.max(0) as u32;
    let clamped_top = top.max(0) as u32;
    let clamped_right = right.max(0) as u32;
    let clamped_bottom = bottom.max(0) as u32;

    let clipped_right = clamped_right.min(visible_width);
    let clipped_bottom = clamped_bottom.min(visible_height);

    if clamped_left >= clipped_right || clamped_top >= clipped_bottom {
        return None;
    }

    Some((clamped_left, clamped_top, clipped_right, clipped_bottom))
}

fn blend_rgba_over(dst: &mut [u8], src: [u8; 4]) {
    let src_alpha = src[3] as f32 / 255.0;
    if src_alpha <= 0.0 {
        return;
    }

    let dst_alpha = dst[3] as f32 / 255.0;
    let out_alpha = src_alpha + dst_alpha * (1.0 - src_alpha);

    for channel in 0..3 {
        let src_channel = src[channel] as f32 / 255.0;
        let dst_channel = dst[channel] as f32 / 255.0;
        let out_channel = if out_alpha == 0.0 {
            0.0
        } else {
            (src_channel * src_alpha + dst_channel * dst_alpha * (1.0 - src_alpha)) / out_alpha
        };
        dst[channel] = (out_channel * 255.0).round().clamp(0.0, 255.0) as u8;
    }

    dst[3] = (out_alpha * 255.0).round().clamp(0.0, 255.0) as u8;
}
