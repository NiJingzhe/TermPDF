use std::cmp::Ordering;
use std::env;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use clap::Parser;
use color_eyre::eyre::{OptionExt, Result, WrapErr, bail};
use pdfium_render::prelude::*;

use crate::document::{
    Document, Glyph, LinkTarget, Page, PageLink, PdfImage, PdfImageAsset, PdfLine,
    PdfMatrix as DocumentPdfMatrix, PdfRect,
};
use crate::pdfium_bundle::{
    bundled_pdfium_variant, packaged_pdfium_library_name, pdfium_extracted_dir,
};
use crate::render::{PageRenderCache, PageRenderPlan, RenderedPage};

const MAX_FRAGMENT_BASELINE_DELTA: f32 = 3.0;
const MAX_FRAGMENT_HORIZONTAL_GAP: f32 = 16.0;
const ANNOTATION_MAX_HEIGHT_FACTOR: f32 = 0.85;
const ANNOTATION_MIN_TARGET_HEIGHT_FACTOR: f32 = 1.15;
const ANNOTATION_MAX_CENTER_DISTANCE_FACTOR: f32 = 1.5;
const ANNOTATION_MAX_VERTICAL_GAP_FACTOR: f32 = 0.6;
const ANNOTATION_MAX_HORIZONTAL_GAP_FACTOR: f32 = 2.0;
const COLUMN_MIN_LINE_WIDTH_RATIO: f32 = 0.25;
const COLUMN_LEFT_MAX_RATIO: f32 = 0.45;
const COLUMN_RIGHT_MIN_RATIO: f32 = 0.50;
const COLUMN_MIN_ALIGNED_ROWS: usize = 3;
const COLUMN_HEADER_MARGIN: f32 = 1.0;
const FULL_WIDTH_LINE_RATIO: f32 = 0.75;
const SPANNING_LINE_MIN_WIDTH_RATIO: f32 = 0.15;
const SPANNING_LINE_MAX_CENTER_OFFSET_RATIO: f32 = 0.08;
const SPANNING_LINE_MIN_HEIGHT_FACTOR: f32 = 1.15;
const SPANNING_LINE_MIN_SUPPORTING_ROWS: usize = 2;
const MAX_IMAGE_OBJECT_DEPTH: usize = 64;

pub struct PdfBackend {
    pdfium: &'static Pdfium,
}

pub struct PdfSession {
    document: Document,
    pdf_document: PdfDocument<'static>,
    render_cache: PageRenderCache,
    pdf_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PdfBackendOptions {
    pub pdf_path: PathBuf,
    pub pdfium_lib_path: Option<PathBuf>,
    pub dark_mode: bool,
    pub watch_mode: bool,
}

#[derive(Parser, Debug)]
#[command(
    name = "termpdf",
    about = "Terminal PDF viewer with kitty image protocol",
    after_help = "Keybindings:\n  hjkl                Move text cursor\n  HJKL                Pan viewport\n  Ctrl-u / Ctrl-d     Half-page up/down\n  Ctrl-b / Ctrl-f     Full-page back/forward\n  gg / {count}gg / G  Jump to page\n  /, n, N, Esc        Search, navigate, hide highlight\n  f / F               Follow visible links\n  Tab / Shift-Tab     Focus next/previous PDF image\n  y                   Copy focused image as PNG\n  v / V / Ctrl-v / y Select text and copy to clipboard\n  m<char> / `<char>   Set and jump to marks\n  F5                  Presentation mode\n  = / - / 0           Zoom in / out / reset\n  i                   Toggle dark mode\n  q                   Quit"
)]
struct CliOptions {
    #[arg(value_name = "FILE")]
    pdf_path: PathBuf,

    #[arg(short = 'w', long = "watch")]
    watch_mode: bool,

    #[arg(long = "pdfium-lib", value_name = "PATH")]
    pdfium_lib_path: Option<PathBuf>,

    #[arg(long = "dark")]
    dark_mode: bool,
}

#[derive(Clone, Debug)]
struct RawGlyph {
    ch: char,
    bbox: PdfRect,
    page: usize,
    source_index: usize,
}

impl PdfBackendOptions {
    pub fn from_args() -> Result<Self> {
        let cli = CliOptions::parse();

        Ok(Self {
            pdf_path: cli.pdf_path,
            watch_mode: cli.watch_mode,
            pdfium_lib_path: cli
                .pdfium_lib_path
                .or_else(|| env::var_os("PDFIUM_LIB_PATH").map(PathBuf::from)),
            dark_mode: cli.dark_mode,
        })
    }

    pub fn from_args_fallback_for_tests<I, T>(
        args: I,
        default_pdfium_lib_path: Option<PathBuf>,
    ) -> Result<Self>
    where
        I: IntoIterator<Item = T>,
        T: Into<PathBuf>,
    {
        Self::parse_for_tests(args, default_pdfium_lib_path)
    }

    fn parse_for_tests<I, T>(args: I, default_pdfium_lib_path: Option<PathBuf>) -> Result<Self>
    where
        I: IntoIterator<Item = T>,
        T: Into<PathBuf>,
    {
        let mut args = args.into_iter().map(Into::into);
        let mut pdf_path = None;
        let mut pdfium_lib_path = default_pdfium_lib_path;
        let mut dark_mode = false;
        let mut watch_mode = false;

        while let Some(arg) = args.next() {
            if arg.as_os_str() == std::ffi::OsStr::new("-w")
                || arg.as_os_str() == std::ffi::OsStr::new("--watch")
            {
                watch_mode = true;
                continue;
            }

            if arg.as_os_str() == std::ffi::OsStr::new("--pdfium-lib") {
                let value = args.next().ok_or_eyre("missing value for --pdfium-lib")?;
                pdfium_lib_path = Some(value);
                continue;
            }

            if arg.as_os_str() == std::ffi::OsStr::new("--dark") {
                dark_mode = true;
                continue;
            }

            if pdf_path.is_some() {
                bail!("unexpected extra argument: {:?}", arg);
            }

            pdf_path = Some(arg);
        }

        let pdf_path = pdf_path.ok_or_eyre(
            "usage: termpdf <file.pdf> [-w|--watch] [--pdfium-lib /path/to/libpdfium-directory]",
        )?;

        Ok(Self {
            pdf_path,
            watch_mode,
            pdfium_lib_path,
            dark_mode,
        })
    }
}

impl PdfBackend {
    pub fn new(pdfium_lib_path: Option<&Path>) -> Result<Self> {
        let resolved_lib_path = resolve_pdfium_lib_path(
            pdfium_lib_path.map(Path::to_path_buf),
            env::var_os("PDFIUM_LIB_PATH").map(PathBuf::from),
            env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            env::consts::OS,
            env::consts::ARCH,
        );

        let bindings = match resolved_lib_path.as_deref() {
            Some(path) => {
                let library_path = if path.extension().is_some() {
                    path.to_path_buf()
                } else {
                    Pdfium::pdfium_platform_library_name_at_path(path)
                };

                Pdfium::bind_to_library(&library_path).wrap_err_with(|| {
                    format!("failed to load Pdfium from {}", library_path.display())
                })?
            }
            None => Pdfium::bind_to_system_library().wrap_err(
                "failed to load Pdfium from system library path; pass --pdfium-lib or set PDFIUM_LIB_PATH",
            )?,
        };

        let pdfium = Box::leak(Box::new(Pdfium::new(bindings)));

        Ok(Self { pdfium })
    }

    pub fn open_session(&self, path: &Path) -> Result<PdfSession> {
        let pdf_document = self
            .pdfium
            .load_pdf_from_file(path, None)
            .wrap_err_with(|| format!("failed to open PDF {}", path.display()))?;
        let document = extract_document(&pdf_document)?;

        Ok(PdfSession {
            document,
            pdf_document,
            render_cache: PageRenderCache::default(),
            pdf_path: path.to_path_buf(),
        })
    }
}

fn resolve_pdfium_lib_path(
    explicit: Option<PathBuf>,
    env_path: Option<PathBuf>,
    project_root: PathBuf,
    os: &str,
    arch: &str,
) -> Option<PathBuf> {
    let packaged_lib_path = env::current_exe().ok().and_then(|exe| {
        let parent = exe.parent()?;
        packaged_pdfium_library_name(os).map(|name| parent.join(name))
    });

    resolve_pdfium_lib_path_for_tests(
        explicit,
        env_path,
        packaged_lib_path,
        project_root,
        os,
        arch,
    )
    .filter(|path| path.exists())
}

fn bundled_pdfium_path(project_root: PathBuf, os: &str, arch: &str) -> Option<PathBuf> {
    let variant = bundled_pdfium_variant(os, arch)?;
    Some(
        pdfium_extracted_dir(project_root, variant)
            .join("lib")
            .join(variant.library_name),
    )
}

pub fn resolve_pdfium_lib_path_for_tests(
    explicit: Option<PathBuf>,
    env_path: Option<PathBuf>,
    packaged_lib_path: Option<PathBuf>,
    project_root: PathBuf,
    os: &str,
    arch: &str,
) -> Option<PathBuf> {
    explicit
        .or(env_path)
        .or(packaged_lib_path)
        .or_else(|| bundled_pdfium_path(project_root, os, arch))
}

impl PdfSession {
    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn pdf_path(&self) -> &Path {
        &self.pdf_path
    }

    pub fn extract_image_assets(&self) -> Result<Vec<PdfImageAsset>> {
        let mut assets = Vec::new();

        for (page_index, page) in self.document.pages.iter().enumerate() {
            for image_index in 0..page.images.len() {
                assets.push(PdfImageAsset {
                    page: page_index,
                    image: image_index,
                    png: self.extract_image_png(page_index, image_index)?,
                });
            }
        }

        Ok(assets)
    }

    pub fn extract_image_png(&self, page_index: usize, image_index: usize) -> Result<Vec<u8>> {
        let image = self
            .document
            .pages
            .get(page_index)
            .and_then(|page| page.images.get(image_index))
            .ok_or_eyre("image index is out of bounds")?;
        let page = self
            .pdf_document
            .pages()
            .get(page_index as i32)
            .wrap_err_with(|| format!("failed to load page {}", page_index + 1))?;

        encode_image_object_png(
            page.objects(),
            &image.object_path,
            &self.pdf_document,
            page_index,
            image_index,
        )
    }

    pub fn render_page(&mut self, plan: PageRenderPlan) -> Result<&RenderedPage> {
        let cache_key = plan.info();

        self.render_cache.get_or_insert_with(cache_key, || {
            let page = self
                .pdf_document
                .pages()
                .get(plan.page_index as i32)
                .wrap_err_with(|| format!("failed to load page {}", plan.page_index + 1))?;

            let bitmap = page
                .render_with_config(
                    &PdfRenderConfig::new()
                        .set_target_size(plan.bitmap_width as i32, plan.bitmap_height as i32),
                )
                .wrap_err_with(|| format!("failed to render page {}", plan.page_index + 1))?;
            let rgba = bitmap.as_rgba_bytes();

            Ok(RenderedPage {
                page_index: cache_key.page_index,
                placement_col: cache_key.placement_col,
                placement_row: cache_key.placement_row,
                bitmap_width: bitmap.width() as u32,
                bitmap_height: bitmap.height() as u32,
                crop_x: cache_key.crop_x,
                crop_y: cache_key.crop_y,
                crop_width: cache_key.crop_width,
                crop_height: cache_key.crop_height,
                placement_columns: cache_key.placement_columns,
                placement_rows: cache_key.placement_rows,
                rgba,
            })
        })
    }
}

fn extract_document(pdf_document: &PdfDocument<'_>) -> Result<Document> {
    let mut pages = Vec::with_capacity(pdf_document.pages().len() as usize);

    for (page_index, page) in pdf_document.pages().iter().enumerate() {
        let page_bbox = PdfRect::new(0.0, 0.0, page.width().value, page.height().value);
        pages.push(Page {
            lines: extract_page_lines(page_index, &page)?,
            bbox: page_bbox,
            links: extract_links(&page),
            images: extract_images(page_index, &page),
        });
    }

    Ok(Document { pages })
}

fn extract_images(page_index: usize, page: &PdfPage<'_>) -> Vec<PdfImage> {
    let mut images = Vec::new();
    collect_images(
        page.objects(),
        page_index,
        pdfium_render::prelude::PdfMatrix::IDENTITY,
        &mut Vec::new(),
        &mut images,
    );
    images.sort_by(compare_images_for_reading_order);
    images
}

fn collect_images<'a, T: PdfPageObjectsCommon<'a>>(
    objects: &'a T,
    page_index: usize,
    parent_matrix: pdfium_render::prelude::PdfMatrix,
    path: &mut Vec<usize>,
    images: &mut Vec<PdfImage>,
) {
    for (object_index, object) in objects.iter().enumerate() {
        path.push(object_index);
        let object_matrix = object
            .matrix()
            .unwrap_or(pdfium_render::prelude::PdfMatrix::IDENTITY);
        let matrix = object_matrix.multiply(parent_matrix);

        if let Some(image) = object.as_image_object()
            && let (Ok(pixel_width), Ok(pixel_height)) = (image.width(), image.height())
        {
            images.push(PdfImage {
                bbox: matrix_bbox(matrix),
                matrix: document_matrix(matrix),
                pixel_width: pixel_width as u32,
                pixel_height: pixel_height as u32,
                page: page_index,
                object_path: path.clone(),
            });
        }

        if path.len() < MAX_IMAGE_OBJECT_DEPTH
            && let Some(form) = object.as_x_object_form_object()
        {
            collect_images(form, page_index, matrix, path, images);
        }
        path.pop();
    }
}

fn encode_image_object_png<'a, T: PdfPageObjectsCommon<'a>>(
    objects: &'a T,
    object_path: &[usize],
    document: &PdfDocument<'_>,
    page_index: usize,
    image_index: usize,
) -> Result<Vec<u8>> {
    let (&object_index, child_path) = object_path
        .split_first()
        .ok_or_eyre("image object path is empty")?;
    let object = objects.get(object_index).wrap_err_with(|| {
        format!(
            "failed to load image object {} on page {}",
            image_index + 1,
            page_index + 1
        )
    })?;

    if child_path.is_empty() {
        let image = object
            .as_image_object()
            .ok_or_eyre("object is not an image")?;
        let dynamic_image = image.get_processed_image(document).wrap_err_with(|| {
            format!(
                "failed to decode image p{}.image{}",
                page_index + 1,
                image_index + 1
            )
        })?;
        let mut png = Vec::new();
        dynamic_image
            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .wrap_err_with(|| {
                format!(
                    "failed to encode image p{}.image{}",
                    page_index + 1,
                    image_index + 1
                )
            })?;
        return Ok(png);
    }

    let form = object
        .as_x_object_form_object()
        .ok_or_eyre("image object path does not point through a form XObject")?;
    encode_image_object_png(form, child_path, document, page_index, image_index)
}

fn matrix_bbox(matrix: pdfium_render::prelude::PdfMatrix) -> PdfRect {
    let points = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
        .map(|(x, y)| matrix.apply_to_points(PdfPoints::new(x), PdfPoints::new(y)));
    let min_x = points
        .iter()
        .map(|(x, _)| x.value)
        .fold(f32::INFINITY, f32::min);
    let min_y = points
        .iter()
        .map(|(_, y)| y.value)
        .fold(f32::INFINITY, f32::min);
    let max_x = points
        .iter()
        .map(|(x, _)| x.value)
        .fold(f32::NEG_INFINITY, f32::max);
    let max_y = points
        .iter()
        .map(|(_, y)| y.value)
        .fold(f32::NEG_INFINITY, f32::max);

    PdfRect::new(min_x, min_y, max_x - min_x, max_y - min_y)
}

fn document_matrix(matrix: pdfium_render::prelude::PdfMatrix) -> DocumentPdfMatrix {
    DocumentPdfMatrix {
        a: matrix.a(),
        b: matrix.b(),
        c: matrix.c(),
        d: matrix.d(),
        e: matrix.e(),
        f: matrix.f(),
    }
}

fn compare_images_for_reading_order(left: &PdfImage, right: &PdfImage) -> Ordering {
    let y_cmp = (right.bbox.y + right.bbox.height)
        .partial_cmp(&(left.bbox.y + left.bbox.height))
        .unwrap_or(Ordering::Equal);
    if y_cmp == Ordering::Equal {
        left.bbox
            .x
            .partial_cmp(&right.bbox.x)
            .unwrap_or(Ordering::Equal)
    } else {
        y_cmp
    }
}

fn extract_links(page: &PdfPage<'_>) -> Vec<PageLink> {
    page.links()
        .iter()
        .filter_map(|link| {
            let rect = link.rect().ok()?;
            let bbox = PdfRect::new(
                rect.left().value,
                rect.bottom().value,
                rect.width().value,
                rect.height().value,
            );

            if let Some(destination) = link.destination() {
                let page = destination.page_index().ok()? as usize;
                let (x, y, zoom) = match destination.view_settings().ok()? {
                    PdfDestinationViewSettings::SpecificCoordinatesAndZoom(x, y, zoom) => {
                        (x.map(|value| value.value), y.map(|value| value.value), zoom)
                    }
                    _ => (None, None, None),
                };

                return Some(PageLink {
                    bbox,
                    target: LinkTarget::LocalDestination { page, x, y, zoom },
                });
            }

            let action = link.action()?;
            if let Some(destination) = action.as_local_destination_action() {
                let destination = destination.destination().ok()?;
                let page = destination.page_index().ok()? as usize;
                let (x, y, zoom) = match destination.view_settings().ok()? {
                    PdfDestinationViewSettings::SpecificCoordinatesAndZoom(x, y, zoom) => {
                        (x.map(|value| value.value), y.map(|value| value.value), zoom)
                    }
                    _ => (None, None, None),
                };

                return Some(PageLink {
                    bbox,
                    target: LinkTarget::LocalDestination { page, x, y, zoom },
                });
            }

            action.as_uri_action().and_then(|uri| {
                uri.uri().ok().map(|value| PageLink {
                    bbox,
                    target: LinkTarget::ExternalUri(value),
                })
            })
        })
        .collect()
}

#[derive(Clone, Debug)]
struct TextFragment {
    bbox: PdfRect,
    glyphs: Vec<RawGlyph>,
    source_index: usize,
    from_segment: bool,
}

#[derive(Clone, Debug)]
struct RawLine {
    fragments: Vec<TextFragment>,
    bbox: PdfRect,
    baseline: f32,
    height: f32,
    source_index: usize,
}

fn extract_page_lines(page_index: usize, page: &PdfPage<'_>) -> Result<Vec<PdfLine>> {
    let text = page
        .text()
        .wrap_err_with(|| format!("failed to extract text for page {}", page_index + 1))?;
    let page_glyphs = text
        .chars()
        .iter()
        .filter_map(|character| raw_glyph_from_pdfium(page_index, character).transpose())
        .collect::<Result<Vec<_>>>()?;
    let mut fragments = text
        .segments()
        .iter()
        .enumerate()
        .map(|(source_index, segment)| {
            let bounds = segment.bounds();
            TextFragment {
                bbox: PdfRect::new(
                    bounds.left().value,
                    bounds.bottom().value,
                    bounds.width().value,
                    bounds.height().value,
                ),
                glyphs: Vec::new(),
                source_index,
                from_segment: true,
            }
        })
        .collect::<Vec<_>>();
    let mut fallback_glyphs = Vec::new();

    for glyph in page_glyphs {
        let candidate = fragments
            .iter()
            .enumerate()
            .filter_map(|(index, fragment)| {
                fragment_membership_score(fragment.bbox, glyph.bbox).map(|score| (index, score))
            })
            .min_by(|(_, left), (_, right)| left.partial_cmp(right).unwrap_or(Ordering::Equal))
            .map(|(index, _)| index);

        if let Some(index) = candidate {
            fragments[index].glyphs.push(glyph);
        } else {
            fallback_glyphs.push(glyph);
        }
    }

    fragments.retain(|fragment| !fragment.glyphs.is_empty());
    for glyph in fallback_glyphs {
        fragments.push(TextFragment {
            bbox: glyph.bbox,
            source_index: glyph.source_index,
            glyphs: vec![glyph],
            from_segment: false,
        });
    }

    let lines = group_text_fragments_into_lines(fragments);
    let lines = order_text_lines_for_reading(lines, page.width().value);
    Ok(lines.into_iter().map(raw_line_to_pdf_line).collect())
}

fn fragment_membership_score(fragment: PdfRect, glyph: PdfRect) -> Option<f32> {
    let vertical_gap = vertical_gap(fragment, glyph);
    let horizontal_gap = horizontal_gap(fragment, glyph);
    let height = fragment.height.min(glyph.height.max(1.0));
    if vertical_gap > MAX_FRAGMENT_BASELINE_DELTA.max(height * 0.75)
        || horizontal_gap > (glyph.width * 1.5).max(4.0)
    {
        return None;
    }

    Some(
        vertical_gap * 10_000.0
            + horizontal_gap * 100.0
            + (rect_center_y(fragment) - rect_center_y(glyph)).abs(),
    )
}

fn raw_glyph_from_pdfium(page_index: usize, char: PdfPageTextChar<'_>) -> Result<Option<RawGlyph>> {
    let Some(ch) = char.unicode_char() else {
        return Ok(None);
    };

    if ch == '\0' || ch == '\r' || ch == '\n' {
        return Ok(None);
    }

    let is_hyphen = char.is_hyphen().wrap_err_with(|| {
        format!(
            "failed to read hyphen flag for page {} char index {}",
            page_index + 1,
            char.index()
        )
    })?;
    let ch = if is_hyphen || ch == '\u{2}' {
        '-'
    } else if ch.is_control() {
        return Ok(None);
    } else {
        ch
    };

    let bounds = char.loose_bounds().wrap_err_with(|| {
        format!(
            "failed to read bounds for page {} char index {}",
            page_index + 1,
            char.index()
        )
    })?;

    Ok(Some(RawGlyph {
        ch,
        bbox: PdfRect::new(
            bounds.left().value,
            bounds.bottom().value,
            bounds.width().value,
            bounds.height().value,
        ),
        page: page_index,
        source_index: char.index(),
    }))
}

#[cfg(test)]
fn group_glyphs_into_lines(mut glyphs: Vec<RawGlyph>) -> Vec<PdfLine> {
    let fragments = glyphs
        .drain(..)
        .enumerate()
        .map(|(source_index, glyph)| TextFragment {
            bbox: glyph.bbox,
            glyphs: vec![glyph],
            source_index,
            from_segment: false,
        })
        .collect();
    order_text_lines_for_reading(group_text_fragments_into_lines(fragments), 595.0)
        .into_iter()
        .map(raw_line_to_pdf_line)
        .collect()
}

fn group_text_fragments_into_lines(mut fragments: Vec<TextFragment>) -> Vec<RawLine> {
    fragments.sort_by(|left, right| {
        right
            .bbox
            .y
            .partial_cmp(&left.bbox.y)
            .unwrap_or(Ordering::Equal)
            .then_with(|| {
                left.bbox
                    .x
                    .partial_cmp(&right.bbox.x)
                    .unwrap_or(Ordering::Equal)
            })
            .then_with(|| left.source_index.cmp(&right.source_index))
    });

    let mut lines = Vec::new();
    for fragment in fragments {
        let candidate = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| same_visual_line(line, &fragment))
            .min_by(|(_, left), (_, right)| {
                baseline_distance(left, &fragment)
                    .partial_cmp(&baseline_distance(right, &fragment))
                    .unwrap_or(Ordering::Equal)
                    .then_with(|| {
                        horizontal_gap(left.bbox, fragment.bbox)
                            .partial_cmp(&horizontal_gap(right.bbox, fragment.bbox))
                            .unwrap_or(Ordering::Equal)
                    })
            })
            .map(|(index, _)| index);

        if let Some(index) = candidate {
            lines[index].fragments.push(fragment);
            update_raw_line(&mut lines[index]);
        } else {
            let line = RawLine {
                baseline: fragment.bbox.y,
                height: fragment.bbox.height.max(1.0),
                bbox: fragment.bbox,
                source_index: fragment.source_index,
                fragments: vec![fragment],
            };
            lines.push(line);
        }
    }

    merge_compatible_line_clusters(&mut lines);
    merge_inline_annotation_lines(&mut lines);
    lines
}

fn same_visual_line(line: &RawLine, fragment: &TextFragment) -> bool {
    same_line_geometry(
        line.bbox,
        line.baseline,
        line.height,
        fragment.bbox,
        fragment.bbox.y,
        fragment.bbox.height.max(1.0),
    )
}

fn same_line_geometry(
    left_bbox: PdfRect,
    left_baseline: f32,
    left_height: f32,
    right_bbox: PdfRect,
    right_baseline: f32,
    right_height: f32,
) -> bool {
    let height = left_height.min(right_height).max(1.0);
    let baseline_tolerance = MAX_FRAGMENT_BASELINE_DELTA.max(height * 0.4);
    let center_tolerance = (height * 0.5).max(1.5);
    if (left_baseline - right_baseline).abs() > baseline_tolerance
        && (rect_center_y(left_bbox) - rect_center_y(right_bbox)).abs() > center_tolerance
    {
        return false;
    }

    let gap = horizontal_gap(left_bbox, right_bbox);
    let horizontal_tolerance = MAX_FRAGMENT_HORIZONTAL_GAP.min((height * 1.6).max(8.0));
    gap <= horizontal_tolerance || horizontal_overlap(left_bbox, right_bbox) > 0.0
}

fn merge_compatible_line_clusters(lines: &mut Vec<RawLine>) {
    loop {
        let candidate = (0..lines.len())
            .flat_map(|left| ((left + 1)..lines.len()).map(move |right| (left, right)))
            .filter(|(left, right)| {
                same_line_geometry(
                    lines[*left].bbox,
                    lines[*left].baseline,
                    lines[*left].height,
                    lines[*right].bbox,
                    lines[*right].baseline,
                    lines[*right].height,
                )
            })
            .min_by(|(left_a, right_a), (left_b, right_b)| {
                horizontal_gap(lines[*left_a].bbox, lines[*right_a].bbox)
                    .partial_cmp(&horizontal_gap(lines[*left_b].bbox, lines[*right_b].bbox))
                    .unwrap_or(Ordering::Equal)
                    .then_with(|| {
                        (rect_center_y(lines[*left_a].bbox) - rect_center_y(lines[*right_a].bbox))
                            .abs()
                            .partial_cmp(
                                &(rect_center_y(lines[*left_b].bbox)
                                    - rect_center_y(lines[*right_b].bbox))
                                .abs(),
                            )
                            .unwrap_or(Ordering::Equal)
                    })
            });

        let Some((left, right)) = candidate else {
            break;
        };
        let merged = lines.remove(right);
        lines[left].fragments.extend(merged.fragments);
        update_raw_line(&mut lines[left]);
    }
}

fn update_raw_line(line: &mut RawLine) {
    let baselines = line
        .fragments
        .iter()
        .map(|fragment| fragment.bbox.y)
        .collect::<Vec<_>>();
    let heights = line
        .fragments
        .iter()
        .map(|fragment| fragment.bbox.height.max(1.0))
        .collect::<Vec<_>>();
    line.baseline = median(baselines);
    line.height = median(heights);
    line.bbox = line
        .fragments
        .iter()
        .map(|fragment| fragment.bbox)
        .reduce(union_rect)
        .unwrap_or_default();
    line.source_index = line
        .fragments
        .iter()
        .map(|fragment| fragment.source_index)
        .min()
        .unwrap_or(line.source_index);
}

fn merge_inline_annotation_lines(lines: &mut Vec<RawLine>) {
    let mut index = 0;
    while index < lines.len() {
        let Some(target_index) = inline_annotation_target(lines, index) else {
            index += 1;
            continue;
        };

        let annotation = lines.remove(index);
        let target_index = if target_index > index {
            target_index - 1
        } else {
            target_index
        };
        lines[target_index].fragments.extend(annotation.fragments);
        update_raw_line(&mut lines[target_index]);
    }
}

fn inline_annotation_target(lines: &[RawLine], annotation_index: usize) -> Option<usize> {
    let annotation = &lines[annotation_index];
    if annotation
        .fragments
        .iter()
        .any(|fragment| fragment.from_segment)
    {
        return None;
    }
    let annotation_height = annotation.bbox.height.max(1.0);

    lines
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != annotation_index)
        .filter_map(|(index, candidate)| {
            let candidate_height = candidate.bbox.height.max(1.0);
            if annotation_height > candidate_height * ANNOTATION_MAX_HEIGHT_FACTOR
                || candidate_height < annotation_height * ANNOTATION_MIN_TARGET_HEIGHT_FACTOR
            {
                return None;
            }

            let center_delta =
                (rect_center_y(annotation.bbox) - rect_center_y(candidate.bbox)).abs();
            if center_delta > candidate_height * ANNOTATION_MAX_CENTER_DISTANCE_FACTOR {
                return None;
            }

            let vertical_gap = vertical_gap(annotation.bbox, candidate.bbox);
            if vertical_gap > candidate_height * ANNOTATION_MAX_VERTICAL_GAP_FACTOR {
                return None;
            }

            let horizontal_gap = horizontal_gap(annotation.bbox, candidate.bbox);
            if horizontal_gap > candidate_height * ANNOTATION_MAX_HORIZONTAL_GAP_FACTOR
                && horizontal_overlap(annotation.bbox, candidate.bbox) <= 0.0
            {
                return None;
            }

            Some((index, center_delta, vertical_gap, horizontal_gap))
        })
        .min_by(|left, right| {
            left.1
                .partial_cmp(&right.1)
                .unwrap_or(Ordering::Equal)
                .then_with(|| left.2.partial_cmp(&right.2).unwrap_or(Ordering::Equal))
                .then_with(|| left.3.partial_cmp(&right.3).unwrap_or(Ordering::Equal))
        })
        .map(|(index, _, _, _)| index)
}

fn order_text_lines_for_reading(mut lines: Vec<RawLine>, page_width: f32) -> Vec<RawLine> {
    lines.sort_by(compare_lines_for_visual_order);
    let Some((column_start_y, split_x)) = detect_column_layout(&lines, page_width) else {
        return lines;
    };

    let mut header = Vec::new();
    let mut full_width = Vec::new();
    let mut left_column = Vec::new();
    let mut right_column = Vec::new();
    let mut unclassified = Vec::new();
    let spanning_line_indices = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| {
            line.bbox.y <= column_start_y + COLUMN_HEADER_MARGIN
                && line.bbox.width < page_width * FULL_WIDTH_LINE_RATIO
                && is_short_spanning_line(line, &lines, page_width, split_x)
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();

    for (index, line) in lines.into_iter().enumerate() {
        if line.bbox.y > column_start_y + COLUMN_HEADER_MARGIN {
            header.push(line);
        } else if line.bbox.width >= page_width * FULL_WIDTH_LINE_RATIO
            || spanning_line_indices.contains(&index)
        {
            full_width.push(line);
        } else if line.bbox.x + line.bbox.width <= split_x + COLUMN_HEADER_MARGIN {
            left_column.push(line);
        } else if line.bbox.x >= split_x - COLUMN_HEADER_MARGIN {
            right_column.push(line);
        } else {
            unclassified.push(line);
        }
    }

    header.sort_by(compare_lines_for_visual_order);
    full_width.sort_by(compare_lines_for_visual_order);
    left_column.sort_by(compare_lines_for_visual_order);
    right_column.sort_by(compare_lines_for_visual_order);
    unclassified.sort_by(compare_lines_for_visual_order);

    let mut ordered = header;
    let mut remaining_left = left_column.into_iter().peekable();
    let mut remaining_right = right_column.into_iter().peekable();

    for spanning in full_width {
        let mut before_left = Vec::new();
        let mut before_right = Vec::new();
        while remaining_left
            .peek()
            .is_some_and(|line| line.bbox.y > spanning.bbox.y)
        {
            before_left.push(remaining_left.next().unwrap());
        }
        while remaining_right
            .peek()
            .is_some_and(|line| line.bbox.y > spanning.bbox.y)
        {
            before_right.push(remaining_right.next().unwrap());
        }
        ordered.extend(before_left);
        ordered.extend(before_right);
        ordered.push(spanning);
    }

    ordered.extend(remaining_left);
    ordered.extend(remaining_right);
    ordered.extend(unclassified);
    ordered
}

fn is_short_spanning_line(
    line: &RawLine,
    lines: &[RawLine],
    page_width: f32,
    split_x: f32,
) -> bool {
    if line.bbox.width < page_width * SPANNING_LINE_MIN_WIDTH_RATIO
        || line.bbox.x >= split_x
        || line.bbox.x + line.bbox.width <= split_x
    {
        return false;
    }

    let center_x = line.bbox.x + line.bbox.width / 2.0;
    if (center_x - split_x).abs() > page_width * SPANNING_LINE_MAX_CENTER_OFFSET_RATIO {
        return false;
    }

    let left_support = lines
        .iter()
        .filter(|candidate| {
            candidate.bbox.y < line.bbox.y
                && candidate.bbox.width >= page_width * COLUMN_MIN_LINE_WIDTH_RATIO
                && candidate.bbox.x + candidate.bbox.width <= split_x + COLUMN_HEADER_MARGIN
        })
        .collect::<Vec<_>>();
    let right_support = lines
        .iter()
        .filter(|candidate| {
            candidate.bbox.y < line.bbox.y
                && candidate.bbox.width >= page_width * COLUMN_MIN_LINE_WIDTH_RATIO
                && candidate.bbox.x >= split_x - COLUMN_HEADER_MARGIN
        })
        .collect::<Vec<_>>();
    if left_support.len() < SPANNING_LINE_MIN_SUPPORTING_ROWS
        || right_support.len() < SPANNING_LINE_MIN_SUPPORTING_ROWS
    {
        return false;
    }

    let support_heights = left_support
        .iter()
        .chain(right_support.iter())
        .map(|candidate| candidate.height.max(1.0))
        .collect::<Vec<_>>();
    line.height >= median(support_heights) * SPANNING_LINE_MIN_HEIGHT_FACTOR
}

fn compare_lines_for_visual_order(left: &RawLine, right: &RawLine) -> Ordering {
    right
        .bbox
        .y
        .partial_cmp(&left.bbox.y)
        .unwrap_or(Ordering::Equal)
        .then_with(|| {
            left.bbox
                .x
                .partial_cmp(&right.bbox.x)
                .unwrap_or(Ordering::Equal)
        })
        .then_with(|| left.source_index.cmp(&right.source_index))
}

fn detect_column_layout(lines: &[RawLine], page_width: f32) -> Option<(f32, f32)> {
    let min_line_width = page_width * COLUMN_MIN_LINE_WIDTH_RATIO;
    let left_lines = lines
        .iter()
        .filter(|line| {
            line.bbox.width >= min_line_width && line.bbox.x < page_width * COLUMN_LEFT_MAX_RATIO
        })
        .collect::<Vec<_>>();
    let right_lines = lines
        .iter()
        .filter(|line| {
            line.bbox.width >= min_line_width && line.bbox.x > page_width * COLUMN_RIGHT_MIN_RATIO
        })
        .collect::<Vec<_>>();
    if count_aligned_rows(&left_lines, &right_lines) < COLUMN_MIN_ALIGNED_ROWS {
        return None;
    }

    let mut candidates = left_lines
        .iter()
        .copied()
        .flat_map(|left| {
            right_lines
                .iter()
                .copied()
                .filter(move |right| {
                    (left.baseline - right.baseline).abs() <= left.height.max(right.height) * 1.5
                })
                .map(move |right| (left, right))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|(left, _), (right, _)| {
        right
            .bbox
            .y
            .partial_cmp(&left.bbox.y)
            .unwrap_or(Ordering::Equal)
    });

    candidates
        .into_iter()
        .map(|(left, right)| {
            let split_x = (left.bbox.x + left.bbox.width + right.bbox.x) / 2.0;
            let body_start = left.bbox.y.min(right.bbox.y);
            let heading_window = left.height.max(right.height) * 3.0;
            let column_start = lines
                .iter()
                .filter(|line| {
                    line.bbox.y >= body_start
                        && line.bbox.y <= body_start + heading_window
                        && (line.bbox.x + line.bbox.width <= split_x + COLUMN_HEADER_MARGIN
                            || line.bbox.x >= split_x - COLUMN_HEADER_MARGIN)
                })
                .map(|line| line.bbox.y)
                .fold(body_start, f32::max);
            (column_start, split_x)
        })
        .next()
}

fn count_aligned_rows(left_lines: &[&RawLine], right_lines: &[&RawLine]) -> usize {
    let mut matched_right = vec![false; right_lines.len()];
    left_lines
        .iter()
        .filter(|left| {
            let candidate = right_lines
                .iter()
                .enumerate()
                .filter(|(index, _)| !matched_right[*index])
                .filter_map(|(index, right)| {
                    let distance = (left.baseline - right.baseline).abs();
                    (distance <= left.height.max(right.height) * 1.5).then_some((index, distance))
                })
                .min_by(|left, right| left.1.partial_cmp(&right.1).unwrap_or(Ordering::Equal));
            if let Some((index, _)) = candidate {
                matched_right[index] = true;
                true
            } else {
                false
            }
        })
        .count()
}

fn raw_line_to_pdf_line(mut line: RawLine) -> PdfLine {
    line.fragments.sort_by(|left, right| {
        left.bbox
            .x
            .partial_cmp(&right.bbox.x)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.source_index.cmp(&right.source_index))
    });
    let mut raw_glyphs = Vec::new();
    let mut fallback_glyphs = Vec::new();
    for fragment in line.fragments {
        if fragment.from_segment {
            raw_glyphs.extend(fragment.glyphs);
        } else {
            fallback_glyphs.extend(fragment.glyphs);
        }
    }
    fallback_glyphs.sort_by(|left, right| {
        left.bbox
            .x
            .partial_cmp(&right.bbox.x)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.source_index.cmp(&right.source_index))
    });
    for glyph in fallback_glyphs {
        let center_x = glyph.bbox.x + glyph.bbox.width / 2.0;
        let index = raw_glyphs
            .iter()
            .position(|candidate| candidate.bbox.x + candidate.bbox.width / 2.0 > center_x)
            .unwrap_or(raw_glyphs.len());
        raw_glyphs.insert(index, glyph);
    }
    let glyph_bbox = raw_glyphs.iter().map(|glyph| glyph.bbox).reduce(union_rect);
    let bbox = glyph_bbox
        .map(|glyph_bbox| union_rect(line.bbox, glyph_bbox))
        .unwrap_or(line.bbox);
    let glyphs = raw_glyphs
        .into_iter()
        .map(|glyph| Glyph {
            ch: glyph.ch,
            bbox: glyph.bbox,
            page: glyph.page,
        })
        .collect();

    PdfLine { glyphs, bbox }
}

fn union_rect(left: PdfRect, right: PdfRect) -> PdfRect {
    let min_x = left.x.min(right.x);
    let min_y = left.y.min(right.y);
    let max_x = (left.x + left.width).max(right.x + right.width);
    let max_y = (left.y + left.height).max(right.y + right.height);
    PdfRect::new(min_x, min_y, max_x - min_x, max_y - min_y)
}

fn horizontal_overlap(left: PdfRect, right: PdfRect) -> f32 {
    (left.x + left.width).min(right.x + right.width) - left.x.max(right.x)
}

fn baseline_distance(line: &RawLine, fragment: &TextFragment) -> f32 {
    (line.baseline - fragment.bbox.y).abs()
}

fn median(mut values: Vec<f32>) -> f32 {
    values.sort_by(|left, right| left.partial_cmp(right).unwrap_or(Ordering::Equal));

    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    }
}

fn rect_center_y(rect: PdfRect) -> f32 {
    rect.y + rect.height / 2.0
}

fn vertical_gap(left: PdfRect, right: PdfRect) -> f32 {
    if left.y > right.y + right.height {
        left.y - (right.y + right.height)
    } else if right.y > left.y + left.height {
        right.y - (left.y + left.height)
    } else {
        0.0
    }
}

fn horizontal_gap(left: PdfRect, right: PdfRect) -> f32 {
    if left.x > right.x + right.width {
        left.x - (right.x + right.width)
    } else if right.x > left.x + left.width {
        right.x - (left.x + left.width)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn superscript_cluster_merges_with_source_adjacent_line() {
        let glyphs = vec![
            raw_glyph('A', 0.0, 120.0, 10.0, 10.0, 0),
            raw_glyph('B', 10.0, 120.0, 10.0, 10.0, 1),
            raw_glyph('C', 20.0, 120.0, 10.0, 10.0, 2),
            raw_glyph('D', 30.0, 120.0, 10.0, 10.0, 3),
            raw_glyph('a', 0.0, 100.0, 10.0, 10.0, 4),
            raw_glyph('b', 10.0, 100.0, 10.0, 10.0, 5),
            raw_glyph('1', 20.0, 111.0, 5.0, 6.0, 6),
            raw_glyph('c', 25.0, 100.0, 10.0, 10.0, 7),
            raw_glyph('d', 35.0, 100.0, 10.0, 10.0, 8),
        ];

        let lines = group_glyphs_into_lines(glyphs);

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text(), "ABCD");
        assert_eq!(lines[1].text(), "ab1cd");
    }

    #[test]
    fn subscript_cluster_merges_with_source_adjacent_line() {
        let glyphs = vec![
            raw_glyph('H', 0.0, 100.0, 10.0, 10.0, 0),
            raw_glyph('2', 10.0, 96.0, 5.0, 6.0, 1),
            raw_glyph('O', 20.0, 100.0, 10.0, 10.0, 2),
        ];

        let lines = group_glyphs_into_lines(glyphs);

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text(), "H2O");
    }

    #[test]
    fn distant_small_text_stays_separate() {
        let mut glyphs = raw_text_line("body", 120.0, 10.0, 0);
        glyphs.extend(raw_text_line("cap", 80.0, 6.0, 4));

        let lines = group_glyphs_into_lines(glyphs);

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text(), "body");
        assert_eq!(lines[1].text(), "cap");
    }

    #[test]
    fn output_line_bbox_contains_assigned_glyphs() {
        let line = RawLine {
            fragments: vec![TextFragment {
                bbox: PdfRect::new(10.0, 100.0, 10.0, 10.0),
                glyphs: vec![raw_glyph('x', 8.0, 98.0, 14.0, 14.0, 0)],
                source_index: 0,
                from_segment: false,
            }],
            bbox: PdfRect::new(10.0, 100.0, 10.0, 10.0),
            baseline: 100.0,
            height: 10.0,
            source_index: 0,
        };

        let output = raw_line_to_pdf_line(line);

        assert_eq!(output.bbox, PdfRect::new(8.0, 98.0, 14.0, 14.0));
    }

    #[test]
    fn fallback_annotation_is_inserted_inside_segment_text() {
        let body_glyphs = vec![
            raw_glyph('a', 0.0, 100.0, 10.0, 10.0, 0),
            raw_glyph('b', 10.0, 100.0, 10.0, 10.0, 1),
            raw_glyph('c', 25.0, 100.0, 10.0, 10.0, 3),
            raw_glyph('d', 35.0, 100.0, 10.0, 10.0, 4),
        ];
        let line = RawLine {
            fragments: vec![
                TextFragment {
                    bbox: PdfRect::new(0.0, 100.0, 45.0, 10.0),
                    glyphs: body_glyphs,
                    source_index: 0,
                    from_segment: true,
                },
                TextFragment {
                    bbox: PdfRect::new(20.0, 111.0, 5.0, 6.0),
                    glyphs: vec![raw_glyph('1', 20.0, 111.0, 5.0, 6.0, 2)],
                    source_index: 2,
                    from_segment: false,
                },
            ],
            bbox: PdfRect::new(0.0, 100.0, 45.0, 17.0),
            baseline: 100.0,
            height: 10.0,
            source_index: 0,
        };

        assert_eq!(raw_line_to_pdf_line(line).text(), "ab1cd");
    }

    #[test]
    fn one_aligned_pair_does_not_trigger_column_layout() {
        let lines = vec![
            raw_line("left table cell", 50.0, 100.0, 0),
            raw_line("right table cell", 330.0, 100.0, 100),
        ];

        assert!(detect_column_layout(&lines, 595.0).is_none());
    }

    #[test]
    fn two_aligned_rows_do_not_trigger_column_layout() {
        let lines = vec![
            raw_line("left table row one", 50.0, 100.0, 0),
            raw_line("right table row one", 330.0, 100.0, 100),
            raw_line("left table row two", 50.0, 80.0, 200),
            raw_line("right table row two", 330.0, 80.0, 300),
        ];

        assert!(detect_column_layout(&lines, 595.0).is_none());
    }

    #[test]
    fn spanning_line_is_emitted_between_column_regions() {
        let mut lines = Vec::new();
        for (index, y) in [600.0, 580.0, 560.0].into_iter().enumerate() {
            lines.push(raw_line(
                &format!("left column body line {index}"),
                50.0,
                y,
                index,
            ));
            lines.push(raw_line(
                &format!("right column body line {index}"),
                330.0,
                y,
                index + 10,
            ));
        }
        lines.push(raw_line(
            "A section heading spanning both columns across the entire page",
            50.0,
            500.0,
            30,
        ));

        let ordered = order_text_lines_for_reading(lines, 595.0);

        assert_eq!(
            ordered.iter().map(raw_line_text).collect::<Vec<_>>(),
            [
                "left column body line 0",
                "left column body line 1",
                "left column body line 2",
                "right column body line 0",
                "right column body line 1",
                "right column body line 2",
                "A section heading spanning both columns across the entire page",
            ]
        );
    }

    #[test]
    fn short_centered_heading_is_emitted_between_column_regions() {
        let mut lines = Vec::new();
        for (index, y) in [600.0, 580.0, 560.0, 540.0, 520.0].into_iter().enumerate() {
            lines.push(raw_line(
                &format!("left body text line {index}"),
                50.0,
                y,
                index,
            ));
            lines.push(raw_line(
                &format!("right body text line {index}"),
                330.0,
                y,
                index + 20,
            ));
        }
        lines.push(raw_line_with_height(
            "Section title",
            235.0,
            550.0,
            12.0,
            40,
        ));

        let ordered = order_text_lines_for_reading(lines, 595.0);

        assert_eq!(
            ordered.iter().map(raw_line_text).collect::<Vec<_>>(),
            [
                "left body text line 0",
                "left body text line 1",
                "left body text line 2",
                "right body text line 0",
                "right body text line 1",
                "right body text line 2",
                "Section title",
                "left body text line 3",
                "left body text line 4",
                "right body text line 3",
                "right body text line 4",
            ]
        );
    }

    #[test]
    fn short_centered_same_size_line_is_not_promoted_to_spanning_region() {
        let mut lines = Vec::new();
        for (index, y) in [600.0, 580.0, 560.0, 540.0].into_iter().enumerate() {
            lines.push(raw_line(
                &format!("left body text line {index}"),
                50.0,
                y,
                index,
            ));
            lines.push(raw_line(
                &format!("right body text line {index}"),
                330.0,
                y,
                index + 20,
            ));
        }
        lines.push(raw_line("Table row across gutter", 170.0, 550.0, 40));

        let ordered = order_text_lines_for_reading(lines, 595.0);

        assert_eq!(
            ordered.last().map(raw_line_text),
            Some("Table row across gutter".to_owned())
        );
    }

    fn raw_text_line(text: &str, y: f32, height: f32, source_index_offset: usize) -> Vec<RawGlyph> {
        text.chars()
            .enumerate()
            .map(|(index, ch)| {
                raw_glyph(
                    ch,
                    index as f32 * 10.0,
                    y,
                    10.0,
                    height,
                    source_index_offset + index,
                )
            })
            .collect()
    }

    fn raw_line(text: &str, x: f32, y: f32, source_index: usize) -> RawLine {
        raw_line_with_height(text, x, y, 10.0, source_index)
    }

    fn raw_line_with_height(
        text: &str,
        x: f32,
        y: f32,
        height: f32,
        source_index: usize,
    ) -> RawLine {
        let glyphs = text
            .chars()
            .enumerate()
            .map(|(index, ch)| {
                raw_glyph(
                    ch,
                    x + index as f32 * 10.0,
                    y,
                    10.0,
                    height,
                    source_index + index,
                )
            })
            .collect::<Vec<_>>();
        let bbox = glyphs
            .iter()
            .map(|glyph| glyph.bbox)
            .reduce(union_rect)
            .unwrap();
        RawLine {
            fragments: vec![TextFragment {
                bbox,
                glyphs,
                source_index,
                from_segment: false,
            }],
            bbox,
            baseline: y,
            height,
            source_index,
        }
    }

    fn raw_line_text(line: &RawLine) -> String {
        line.fragments
            .iter()
            .flat_map(|fragment| fragment.glyphs.iter().map(|glyph| glyph.ch))
            .collect()
    }

    fn raw_glyph(
        ch: char,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        source_index: usize,
    ) -> RawGlyph {
        RawGlyph {
            ch,
            bbox: PdfRect::new(x, y, width, height),
            page: 0,
            source_index,
        }
    }
}
