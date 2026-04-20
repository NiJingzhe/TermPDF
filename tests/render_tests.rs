use ratatui::layout::Rect;
use termpdf::document::{Document, Page, PdfRect};
use termpdf::render::{
    build_document_layout, build_highlight_mask, build_page_render_plan, build_visible_page_plans,
    compose_visible_page_frame, compose_visible_page_frame_with_offsets, current_page_for_scroll,
    invert_rgba_in_place, viewport_pixels, CellPixels, FrameOffsets, OverlayPlacement,
    PageRenderCache, PageRenderInfo, RenderedPage, ViewportOffset, ViewportPixels,
    ACTIVE_SEARCH_HIGHLIGHT_RGBA, SEARCH_HIGHLIGHT_RGBA,
};

#[test]
fn viewport_pixels_use_terminal_pixel_metrics_when_available() {
    let viewport = viewport_pixels(
        Rect::new(2, 3, 80, 20),
        crossterm::terminal::WindowSize {
            rows: 50,
            columns: 200,
            width: 1600,
            height: 1000,
        },
    );

    assert_eq!(
        viewport.cell,
        CellPixels {
            width: 8,
            height: 20
        }
    );
    assert_eq!(viewport.width, 640);
    assert_eq!(viewport.height, 400);
}

#[test]
fn viewport_pixels_fall_back_when_terminal_reports_zero_pixels() {
    let viewport = viewport_pixels(
        Rect::new(0, 0, 10, 5),
        crossterm::terminal::WindowSize {
            rows: 40,
            columns: 120,
            width: 0,
            height: 0,
        },
    );

    assert_eq!(
        viewport.cell,
        CellPixels {
            width: 8,
            height: 16
        }
    );
    assert_eq!(viewport.width, 80);
    assert_eq!(viewport.height, 80);
}

#[test]
fn build_page_render_plan_preserves_page_aspect_ratio() {
    let viewport = ViewportPixels {
        area: Rect::new(1, 1, 100, 20),
        cell: CellPixels {
            width: 8,
            height: 20,
        },
        width: 800,
        height: 400,
    };

    let plan = build_page_render_plan(
        2,
        PdfRect::new(0.0, 0.0, 600.0, 800.0),
        viewport,
        1.0,
        ViewportOffset::default(),
    )
    .unwrap();

    assert_eq!(plan.page_index, 2);
    assert_eq!(plan.bitmap_width, 300);
    assert_eq!(plan.bitmap_height, 400);
    assert_eq!(plan.crop_width, 800);
    assert_eq!(plan.crop_height, 400);
    assert_eq!(plan.placement_columns, 100);
    assert_eq!(plan.placement_rows, 20);
    assert_eq!(plan.placement_col, 1);
    assert_eq!(plan.placement_row, 1);
}

#[test]
fn build_page_render_plan_zooms_bitmap_but_keeps_same_visible_crop() {
    let viewport = ViewportPixels {
        area: Rect::new(1, 1, 100, 20),
        cell: CellPixels {
            width: 8,
            height: 20,
        },
        width: 800,
        height: 400,
    };

    let plan = build_page_render_plan(
        2,
        PdfRect::new(0.0, 0.0, 600.0, 800.0),
        viewport,
        2.0,
        ViewportOffset::default(),
    )
    .unwrap();

    assert_eq!(plan.bitmap_width, 600);
    assert_eq!(plan.bitmap_height, 800);
    assert_eq!(plan.crop_width, 800);
    assert_eq!(plan.crop_height, 400);
    assert_eq!(plan.placement_columns, 100);
    assert_eq!(plan.placement_rows, 20);
    assert_eq!(plan.placement_col, 1);
    assert_eq!(plan.placement_row, 1);
}

#[test]
fn build_page_render_plan_scales_down_below_100_percent() {
    let viewport = ViewportPixels {
        area: Rect::new(1, 1, 100, 20),
        cell: CellPixels {
            width: 8,
            height: 20,
        },
        width: 800,
        height: 400,
    };

    let plan = build_page_render_plan(
        2,
        PdfRect::new(0.0, 0.0, 600.0, 800.0),
        viewport,
        0.5,
        ViewportOffset::default(),
    )
    .unwrap();

    assert_eq!(plan.bitmap_width, 150);
    assert_eq!(plan.bitmap_height, 200);
    assert_eq!(plan.crop_width, 800);
    assert_eq!(plan.crop_height, 400);
    assert_eq!(plan.placement_columns, 100);
    assert_eq!(plan.placement_rows, 20);
}

#[test]
fn build_page_render_plan_applies_viewport_offset_and_clamps_crop_origin() {
    let viewport = ViewportPixels {
        area: Rect::new(1, 1, 100, 20),
        cell: CellPixels {
            width: 8,
            height: 20,
        },
        width: 800,
        height: 400,
    };

    let plan = build_page_render_plan(
        2,
        PdfRect::new(0.0, 0.0, 600.0, 800.0),
        viewport,
        2.0,
        ViewportOffset { x: 900, y: 700 },
    )
    .unwrap();

    assert_eq!(plan.bitmap_width, 600);
    assert_eq!(plan.bitmap_height, 800);
    assert_eq!(plan.crop_width, 800);
    assert_eq!(plan.crop_height, 400);
    assert_eq!(plan.crop_x, 0);
    assert_eq!(plan.crop_y, 400);
    assert_eq!(plan.placement_col, 1);
    assert_eq!(plan.placement_row, 1);
}

#[test]
fn build_page_render_plan_centers_page_inside_viewport() {
    let viewport = ViewportPixels {
        area: Rect::new(10, 5, 30, 20),
        cell: CellPixels {
            width: 8,
            height: 16,
        },
        width: 240,
        height: 320,
    };

    let plan = build_page_render_plan(
        0,
        PdfRect::new(0.0, 0.0, 100.0, 100.0),
        viewport,
        1.0,
        ViewportOffset::default(),
    )
    .unwrap();

    assert_eq!(plan.crop_width, 240);
    assert_eq!(plan.crop_height, 320);
    assert_eq!(plan.placement_columns, 30);
    assert_eq!(plan.placement_rows, 20);
    assert_eq!(plan.placement_col, 10);
    assert_eq!(plan.placement_row, 5);
}

#[test]
fn build_page_render_plan_zoom_is_anchored_to_viewport_center() {
    let viewport = ViewportPixels {
        area: Rect::new(0, 0, 100, 20),
        cell: CellPixels {
            width: 8,
            height: 20,
        },
        width: 800,
        height: 400,
    };

    let plan = build_page_render_plan(
        0,
        PdfRect::new(0.0, 0.0, 600.0, 800.0),
        viewport,
        2.0,
        ViewportOffset { x: 0, y: 0 },
    )
    .unwrap();

    assert_eq!(plan.crop_width, 800);
    assert_eq!(plan.crop_height, 400);
    assert_eq!(plan.crop_x, 0);
    assert_eq!(plan.crop_y, 0);
}

#[test]
fn build_document_layout_stacks_pages_vertically() {
    let document = Document {
        pages: vec![
            Page::from_text(0, &["alpha", "beta"]),
            Page::from_text(1, &["gamma"]),
        ],
    };
    let viewport = ViewportPixels {
        area: Rect::new(0, 0, 100, 20),
        cell: CellPixels {
            width: 8,
            height: 20,
        },
        width: 800,
        height: 400,
    };

    let layout = build_document_layout(&document, viewport, 1.0);

    assert_eq!(layout.pages.len(), 2);
    assert_eq!(layout.pages[0].doc_y, 0);
    assert!(layout.pages[1].doc_y > layout.pages[0].bitmap_height);
    assert!(layout.total_height >= layout.pages[1].doc_y + layout.pages[1].bitmap_height);
}

#[test]
fn current_page_for_scroll_uses_viewport_center() {
    let document = Document {
        pages: vec![
            Page::from_text(0, &["alpha", "beta"]),
            Page::from_text(1, &["gamma"]),
        ],
    };
    let viewport = ViewportPixels {
        area: Rect::new(0, 0, 100, 20),
        cell: CellPixels {
            width: 8,
            height: 20,
        },
        width: 800,
        height: 400,
    };
    let layout = build_document_layout(&document, viewport, 1.0);

    assert_eq!(current_page_for_scroll(&layout, 0, viewport.height), 0);
    assert_eq!(
        current_page_for_scroll(&layout, layout.pages[1].doc_y, viewport.height),
        1
    );
}

#[test]
fn build_visible_page_plans_only_returns_pages_with_visible_clipped_region() {
    let document = Document {
        pages: (0..12)
            .map(|page| Page::from_text(page, &["alpha", "beta"]))
            .collect(),
    };
    let viewport = ViewportPixels {
        area: Rect::new(0, 0, 100, 20),
        cell: CellPixels {
            width: 8,
            height: 20,
        },
        width: 800,
        height: 400,
    };
    let layout = build_document_layout(&document, viewport, 1.0);
    let scroll_y = layout.pages[6].doc_y;

    let plans = build_visible_page_plans(&layout, viewport, ViewportOffset { x: 0, y: scroll_y });

    assert_eq!(plans.len(), 1);
    assert!(plans.iter().any(|plan| plan.page_index == 6));
}

#[test]
fn render_cache_reuses_existing_bitmap_for_same_plan() {
    let key = PageRenderInfo {
        page_index: 0,
        placement_col: 0,
        placement_row: 0,
        bitmap_width: 640,
        bitmap_height: 480,
        crop_x: 0,
        crop_y: 0,
        crop_width: 640,
        crop_height: 480,
        placement_columns: 80,
        placement_rows: 30,
    };
    let mut cache = PageRenderCache::default();
    let mut renders = 0;

    let first = cache
        .get_or_insert_with(key, || {
            renders += 1;
            Ok::<_, ()>(RenderedPage {
                page_index: 0,
                placement_col: 0,
                placement_row: 0,
                bitmap_width: 640,
                bitmap_height: 480,
                crop_x: 0,
                crop_y: 0,
                crop_width: 640,
                crop_height: 480,
                placement_columns: 80,
                placement_rows: 30,
                rgba: vec![1, 2, 3, 4],
            })
        })
        .unwrap()
        .rgba
        .clone();
    let second = cache
        .get_or_insert_with(key, || {
            renders += 1;
            Ok::<_, ()>(RenderedPage {
                page_index: 0,
                placement_col: 0,
                placement_row: 0,
                bitmap_width: 640,
                bitmap_height: 480,
                crop_x: 0,
                crop_y: 0,
                crop_width: 640,
                crop_height: 480,
                placement_columns: 80,
                placement_rows: 30,
                rgba: vec![9, 9, 9, 9],
            })
        })
        .unwrap()
        .rgba
        .clone();

    assert_eq!(renders, 1);
    assert_eq!(first, second);
}

#[test]
fn render_cache_misses_when_target_size_changes() {
    let first_key = PageRenderInfo {
        page_index: 0,
        placement_col: 0,
        placement_row: 0,
        bitmap_width: 640,
        bitmap_height: 480,
        crop_x: 0,
        crop_y: 0,
        crop_width: 640,
        crop_height: 480,
        placement_columns: 80,
        placement_rows: 30,
    };
    let second_key = PageRenderInfo {
        page_index: 0,
        placement_col: 0,
        placement_row: 0,
        bitmap_width: 800,
        bitmap_height: 600,
        crop_x: 0,
        crop_y: 0,
        crop_width: 800,
        crop_height: 600,
        placement_columns: 100,
        placement_rows: 38,
    };
    let mut cache = PageRenderCache::default();
    let mut renders = 0;

    cache
        .get_or_insert_with(first_key, || {
            renders += 1;
            Ok::<_, ()>(RenderedPage {
                page_index: 0,
                placement_col: 0,
                placement_row: 0,
                bitmap_width: 640,
                bitmap_height: 480,
                crop_x: 0,
                crop_y: 0,
                crop_width: 640,
                crop_height: 480,
                placement_columns: 80,
                placement_rows: 30,
                rgba: vec![1, 2, 3, 4],
            })
        })
        .unwrap();
    cache
        .get_or_insert_with(second_key, || {
            renders += 1;
            Ok::<_, ()>(RenderedPage {
                page_index: 0,
                placement_col: 0,
                placement_row: 0,
                bitmap_width: 800,
                bitmap_height: 600,
                crop_x: 0,
                crop_y: 0,
                crop_width: 800,
                crop_height: 600,
                placement_columns: 100,
                placement_rows: 38,
                rgba: vec![5, 6, 7, 8],
            })
        })
        .unwrap();

    assert_eq!(renders, 2);
}

#[test]
fn render_cache_evicts_least_recently_used_entry() {
    let mut cache = PageRenderCache::with_capacity(2);
    let first = PageRenderInfo {
        page_index: 0,
        placement_col: 0,
        placement_row: 0,
        bitmap_width: 100,
        bitmap_height: 100,
        crop_x: 0,
        crop_y: 0,
        crop_width: 100,
        crop_height: 100,
        placement_columns: 10,
        placement_rows: 10,
    };
    let second = PageRenderInfo {
        page_index: 1,
        placement_col: 0,
        placement_row: 0,
        bitmap_width: 100,
        bitmap_height: 100,
        crop_x: 0,
        crop_y: 0,
        crop_width: 100,
        crop_height: 100,
        placement_columns: 10,
        placement_rows: 10,
    };
    let third = PageRenderInfo {
        page_index: 2,
        placement_col: 0,
        placement_row: 0,
        bitmap_width: 100,
        bitmap_height: 100,
        crop_x: 0,
        crop_y: 0,
        crop_width: 100,
        crop_height: 100,
        placement_columns: 10,
        placement_rows: 10,
    };

    for key in [first, second, third] {
        cache
            .get_or_insert_with(key, || {
                Ok::<_, ()>(RenderedPage {
                    page_index: key.page_index,
                    placement_col: 0,
                    placement_row: 0,
                    bitmap_width: key.bitmap_width,
                    bitmap_height: key.bitmap_height,
                    crop_x: key.crop_x,
                    crop_y: key.crop_y,
                    crop_width: key.crop_width,
                    crop_height: key.crop_height,
                    placement_columns: key.placement_columns,
                    placement_rows: key.placement_rows,
                    rgba: vec![key.page_index as u8],
                })
            })
            .unwrap();
    }

    assert!(!cache.contains(first));
    assert!(cache.contains(second));
    assert!(cache.contains(third));
}

#[test]
fn build_highlight_mask_creates_transparent_padding_around_match() {
    let overlay = OverlayPlacement {
        cell_x: 10,
        cell_y: 5,
        columns: 2,
        rows: 1,
        offset_x: 2,
        offset_y: 3,
        width_px: 10,
        height_px: 4,
        cell: CellPixels {
            width: 8,
            height: 10,
        },
    };

    let mask = build_highlight_mask(0, overlay, [1, 2, 3, 4]).unwrap();

    assert_eq!(mask.bitmap_width, 16);
    assert_eq!(mask.bitmap_height, 10);
    assert_eq!(mask.crop_width, 16);
    assert_eq!(mask.crop_height, 10);
    assert_eq!(&mask.rgba[0..4], &[0, 0, 0, 0]);
    let highlighted = (((3u32 * 16) + 2) * 4) as usize;
    assert_eq!(&mask.rgba[highlighted..highlighted + 4], &[1, 2, 3, 4]);
}

#[test]
fn active_highlight_color_is_more_opaque_than_passive() {
    assert!(ACTIVE_SEARCH_HIGHLIGHT_RGBA[3] > SEARCH_HIGHLIGHT_RGBA[3]);
}

#[test]
fn invert_rgba_in_place_flips_rgb_but_preserves_alpha() {
    let mut bytes = vec![10, 20, 30, 40, 0, 128, 255, 77];

    invert_rgba_in_place(&mut bytes);

    assert_eq!(bytes, vec![245, 235, 225, 40, 255, 127, 0, 77]);
}

#[test]
fn compose_visible_page_frame_ignores_highlights_outside_current_crop() {
    let source = RenderedPage {
        page_index: 0,
        placement_col: 0,
        placement_row: 0,
        bitmap_width: 200,
        bitmap_height: 100,
        crop_x: 0,
        crop_y: 0,
        crop_width: 100,
        crop_height: 50,
        placement_columns: 10,
        placement_rows: 5,
        rgba: vec![255; 200 * 100 * 4],
    };

    let frame = compose_visible_page_frame(
        &source,
        PdfRect::new(0.0, 0.0, 200.0, 100.0),
        CellPixels {
            width: 10,
            height: 10,
        },
        false,
        &[PdfRect::new(150.0, 75.0, 20.0, 10.0)],
        None,
        &[],
    );

    assert!(frame.rgba.iter().all(|value| *value == 255));
}

#[test]
fn compose_visible_page_frame_changes_with_crop_at_zoom_over_100() {
    let mut rgba = vec![0u8; 4 * 4 * 4];
    for y in 0..4u32 {
        for x in 0..4u32 {
            let i = ((y * 4 + x) * 4) as usize;
            rgba[i] = (x * 40) as u8;
            rgba[i + 1] = (y * 40) as u8;
            rgba[i + 2] = 0;
            rgba[i + 3] = 255;
        }
    }

    let left = compose_visible_page_frame(
        &RenderedPage {
            page_index: 0,
            placement_col: 0,
            placement_row: 0,
            bitmap_width: 4,
            bitmap_height: 4,
            crop_x: 0,
            crop_y: 0,
            crop_width: 2,
            crop_height: 2,
            placement_columns: 2,
            placement_rows: 2,
            rgba: rgba.clone(),
        },
        PdfRect::new(0.0, 0.0, 4.0, 4.0),
        CellPixels {
            width: 1,
            height: 1,
        },
        false,
        &[],
        None,
        &[],
    );
    let right = compose_visible_page_frame(
        &RenderedPage {
            page_index: 0,
            placement_col: 0,
            placement_row: 0,
            bitmap_width: 4,
            bitmap_height: 4,
            crop_x: 2,
            crop_y: 0,
            crop_width: 2,
            crop_height: 2,
            placement_columns: 2,
            placement_rows: 2,
            rgba,
        },
        PdfRect::new(0.0, 0.0, 4.0, 4.0),
        CellPixels {
            width: 1,
            height: 1,
        },
        false,
        &[],
        None,
        &[],
    );

    assert_ne!(left.rgba, right.rgba);
}

#[test]
fn compose_visible_page_frame_maps_pdf_y_from_bottom_origin() {
    let source = RenderedPage {
        page_index: 0,
        placement_col: 0,
        placement_row: 0,
        bitmap_width: 100,
        bitmap_height: 100,
        crop_x: 0,
        crop_y: 0,
        crop_width: 100,
        crop_height: 100,
        placement_columns: 10,
        placement_rows: 10,
        rgba: vec![0; 100 * 100 * 4],
    };

    let frame = compose_visible_page_frame(
        &source,
        PdfRect::new(0.0, 0.0, 100.0, 100.0),
        CellPixels {
            width: 10,
            height: 10,
        },
        false,
        &[PdfRect::new(0.0, 0.0, 10.0, 10.0)],
        None,
        &[],
    );

    let top_left = 0usize;
    let bottom_left = ((90 * 100) * 4) as usize;
    assert_eq!(&frame.rgba[top_left..top_left + 4], &[0, 0, 0, 0]);
    assert_ne!(&frame.rgba[bottom_left..bottom_left + 4], &[0, 0, 0, 0]);
}

#[test]
fn compose_visible_page_frame_crops_to_visible_region_and_pads_to_cell_boundary() {
    let source = RenderedPage {
        page_index: 0,
        placement_col: 3,
        placement_row: 4,
        bitmap_width: 4,
        bitmap_height: 2,
        crop_x: 1,
        crop_y: 0,
        crop_width: 2,
        crop_height: 1,
        placement_columns: 2,
        placement_rows: 1,
        rgba: vec![
            10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255, 130, 140, 150,
            255, 160, 170, 180, 255, 190, 200, 210, 255, 220, 230, 240, 255,
        ],
    };

    let frame = compose_visible_page_frame(
        &source,
        PdfRect::new(0.0, 0.0, 4.0, 2.0),
        CellPixels {
            width: 2,
            height: 2,
        },
        false,
        &[],
        None,
        &[],
    );

    assert_eq!(frame.bitmap_width, 4);
    assert_eq!(frame.bitmap_height, 2);
    assert_eq!(frame.crop_x, 0);
    assert_eq!(frame.crop_y, 0);
    assert_eq!(frame.crop_width, 4);
    assert_eq!(frame.crop_height, 2);
    assert_eq!(&frame.rgba[0..8], &[0, 0, 0, 0, 40, 50, 60, 255]);
    assert_eq!(&frame.rgba[8..16], &[70, 80, 90, 255, 0, 0, 0, 0]);
}

#[test]
fn compose_visible_page_frame_respects_explicit_frame_offsets() {
    let source = RenderedPage {
        page_index: 0,
        placement_col: 3,
        placement_row: 4,
        bitmap_width: 4,
        bitmap_height: 2,
        crop_x: 1,
        crop_y: 0,
        crop_width: 2,
        crop_height: 1,
        placement_columns: 2,
        placement_rows: 1,
        rgba: vec![
            10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255, 130, 140, 150,
            255, 160, 170, 180, 255, 190, 200, 210, 255, 220, 230, 240, 255,
        ],
    };

    let frame = compose_visible_page_frame_with_offsets(
        &source,
        PdfRect::new(0.0, 0.0, 4.0, 2.0),
        CellPixels {
            width: 2,
            height: 2,
        },
        false,
        &[],
        None,
        &[],
        Some(FrameOffsets { x: 1, y: 0 }),
    );

    assert_eq!(&frame.rgba[4..12], &[40, 50, 60, 255, 70, 80, 90, 255]);
}

#[test]
fn compose_visible_page_frame_centers_narrow_page_in_viewport() {
    let source = RenderedPage {
        page_index: 0,
        placement_col: 10,
        placement_row: 5,
        bitmap_width: 2,
        bitmap_height: 4,
        crop_x: 0,
        crop_y: 0,
        crop_width: 2,
        crop_height: 4,
        placement_columns: 4,
        placement_rows: 4,
        rgba: vec![255, 0, 0, 255].repeat(8),
    };

    let frame = compose_visible_page_frame(
        &source,
        PdfRect::new(0.0, 0.0, 2.0, 4.0),
        CellPixels {
            width: 1,
            height: 1,
        },
        false,
        &[],
        None,
        &[],
    );

    assert_eq!(&frame.rgba[0..4], &[0, 0, 0, 0]);
    assert_eq!(&frame.rgba[4..8], &[255, 0, 0, 255]);
    assert_eq!(&frame.rgba[8..12], &[255, 0, 0, 255]);
    assert_eq!(&frame.rgba[12..16], &[0, 0, 0, 0]);
}

#[test]
fn compose_visible_page_frame_applies_dark_mode_and_search_highlights() {
    let source = RenderedPage {
        page_index: 0,
        placement_col: 0,
        placement_row: 0,
        bitmap_width: 4,
        bitmap_height: 2,
        crop_x: 0,
        crop_y: 0,
        crop_width: 4,
        crop_height: 2,
        placement_columns: 2,
        placement_rows: 1,
        rgba: vec![20, 40, 60, 255].repeat(8),
    };

    let frame = compose_visible_page_frame(
        &source,
        PdfRect::new(0.0, 0.0, 4.0, 2.0),
        CellPixels {
            width: 2,
            height: 2,
        },
        true,
        &[PdfRect::new(0.0, 0.0, 2.0, 1.0)],
        Some(PdfRect::new(2.0, 0.0, 2.0, 1.0)),
        &[],
    );

    assert_eq!(&frame.rgba[0..4], &[235, 215, 195, 255]);
    assert_eq!(&frame.rgba[8..12], &[235, 215, 195, 255]);
    assert_ne!(&frame.rgba[16..20], &[235, 215, 195, 255]);
}
