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

const LINE_CENTER_TOLERANCE_FACTOR: f32 = 0.4;
const LINE_OVERLAP_MIN_RATIO: f32 = 0.65;
const INLINE_ANNOTATION_MAX_HEIGHT_FACTOR: f32 = 0.85;
const INLINE_ANNOTATION_MIN_TARGET_HEIGHT_FACTOR: f32 = 1.15;
const INLINE_ANNOTATION_MAX_CENTER_DISTANCE_FACTOR: f32 = 1.0;
const INLINE_ANNOTATION_MAX_VERTICAL_GAP_FACTOR: f32 = 0.35;
const INLINE_ANNOTATION_MAX_HORIZONTAL_GAP_FACTOR: f32 = 2.0;
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
        let text = page
            .text()
            .wrap_err_with(|| format!("failed to extract text for page {}", page_index + 1))?;
        let raw_glyphs = text
            .chars()
            .iter()
            .filter_map(|char| raw_glyph_from_pdfium(page_index, char).transpose())
            .collect::<Result<Vec<_>>>()?;

        pages.push(Page {
            lines: group_glyphs_into_lines(raw_glyphs),
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

fn raw_glyph_from_pdfium(page_index: usize, char: PdfPageTextChar<'_>) -> Result<Option<RawGlyph>> {
    let Some(ch) = char.unicode_char() else {
        return Ok(None);
    };

    if ch == '\0' || ch == '\r' || ch == '\n' {
        return Ok(None);
    }

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

fn group_glyphs_into_lines(mut glyphs: Vec<RawGlyph>) -> Vec<PdfLine> {
    glyphs.sort_by(compare_glyphs_for_reading_order);

    let mut lines: Vec<Vec<RawGlyph>> = Vec::new();

    for glyph in glyphs {
        if let Some(line) = lines.iter_mut().find(|line| same_visual_line(line, &glyph)) {
            line.push(glyph);
        } else {
            lines.push(vec![glyph]);
        }
    }

    merge_inline_annotation_clusters(&mut lines);
    lines.sort_by(|left, right| compare_line_clusters_for_reading_order(left, right));

    lines.into_iter().map(raw_line_to_pdf_line).collect()
}

fn compare_glyphs_for_reading_order(left: &RawGlyph, right: &RawGlyph) -> Ordering {
    let y_cmp = right
        .bbox
        .y
        .partial_cmp(&left.bbox.y)
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

fn same_visual_line(line: &[RawGlyph], glyph: &RawGlyph) -> bool {
    let line_center = median_glyph_center_y(line);
    let line_height = median_glyph_height(line).max(1.0);
    let glyph_center = rect_center_y(glyph.bbox);
    let glyph_height = glyph.bbox.height.max(1.0);
    let center_delta = (glyph_center - line_center).abs();
    let center_tolerance = (line_height.min(glyph_height) * LINE_CENTER_TOLERANCE_FACTOR).max(1.0);

    if center_delta <= center_tolerance {
        return true;
    }

    let line_band = PdfRect::new(0.0, line_center - line_height / 2.0, 0.0, line_height);
    let overlap = vertical_overlap_ratio(line_band, glyph.bbox);

    overlap >= LINE_OVERLAP_MIN_RATIO && center_delta <= line_height.max(glyph_height) * 0.55
}

fn merge_inline_annotation_clusters(lines: &mut Vec<Vec<RawGlyph>>) {
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
        lines[target_index].extend(annotation);
    }
}

fn inline_annotation_target(lines: &[Vec<RawGlyph>], annotation_index: usize) -> Option<usize> {
    let annotation_bbox = raw_line_bbox(&lines[annotation_index]);
    let annotation_height = annotation_bbox.height.max(1.0);
    let annotation_center = rect_center_y(annotation_bbox);
    let annotation_source_span = source_span(&lines[annotation_index]);

    lines
        .iter()
        .enumerate()
        .filter(|(candidate_index, _)| *candidate_index != annotation_index)
        .filter_map(|(candidate_index, candidate)| {
            let candidate_bbox = raw_line_bbox(candidate);
            let candidate_height = candidate_bbox.height.max(1.0);

            if annotation_height > candidate_height * INLINE_ANNOTATION_MAX_HEIGHT_FACTOR {
                return None;
            }

            if candidate_height < annotation_height * INLINE_ANNOTATION_MIN_TARGET_HEIGHT_FACTOR {
                return None;
            }

            let center_delta = (annotation_center - rect_center_y(candidate_bbox)).abs();
            if center_delta > candidate_height * INLINE_ANNOTATION_MAX_CENTER_DISTANCE_FACTOR {
                return None;
            }

            let vertical_gap = vertical_gap(annotation_bbox, candidate_bbox);
            if vertical_gap > candidate_height * INLINE_ANNOTATION_MAX_VERTICAL_GAP_FACTOR {
                return None;
            }

            let horizontal_gap = horizontal_gap(annotation_bbox, candidate_bbox);
            if horizontal_gap > candidate_height * INLINE_ANNOTATION_MAX_HORIZONTAL_GAP_FACTOR {
                return None;
            }

            let source_gap = source_gap(annotation_source_span, source_span(candidate));
            Some((
                candidate_index,
                source_gap,
                center_delta,
                vertical_gap,
                horizontal_gap,
            ))
        })
        .min_by(|left, right| {
            left.1
                .cmp(&right.1)
                .then_with(|| left.2.partial_cmp(&right.2).unwrap_or(Ordering::Equal))
                .then_with(|| left.3.partial_cmp(&right.3).unwrap_or(Ordering::Equal))
                .then_with(|| left.4.partial_cmp(&right.4).unwrap_or(Ordering::Equal))
        })
        .map(|(candidate_index, _, _, _, _)| candidate_index)
}

fn compare_line_clusters_for_reading_order(left: &[RawGlyph], right: &[RawGlyph]) -> Ordering {
    let y_cmp = median_glyph_center_y(right)
        .partial_cmp(&median_glyph_center_y(left))
        .unwrap_or(Ordering::Equal);

    if y_cmp == Ordering::Equal {
        min_x(left)
            .partial_cmp(&min_x(right))
            .unwrap_or(Ordering::Equal)
    } else {
        y_cmp
    }
}

fn raw_line_to_pdf_line(mut line: Vec<RawGlyph>) -> PdfLine {
    line.sort_by(|left, right| {
        left.bbox
            .x
            .partial_cmp(&right.bbox.x)
            .unwrap_or(Ordering::Equal)
            .then_with(|| {
                right
                    .bbox
                    .y
                    .partial_cmp(&left.bbox.y)
                    .unwrap_or(Ordering::Equal)
            })
    });

    let bbox = raw_line_bbox(&line);
    let glyphs = line
        .into_iter()
        .map(|glyph| Glyph {
            ch: glyph.ch,
            bbox: glyph.bbox,
            page: glyph.page,
        })
        .collect::<Vec<_>>();

    PdfLine { glyphs, bbox }
}

fn raw_line_bbox(line: &[RawGlyph]) -> PdfRect {
    let min_x = line
        .iter()
        .map(|glyph| glyph.bbox.x)
        .fold(f32::INFINITY, f32::min);
    let min_y = line
        .iter()
        .map(|glyph| glyph.bbox.y)
        .fold(f32::INFINITY, f32::min);
    let max_x = line
        .iter()
        .map(|glyph| glyph.bbox.x + glyph.bbox.width)
        .fold(f32::NEG_INFINITY, f32::max);
    let max_y = line
        .iter()
        .map(|glyph| glyph.bbox.y + glyph.bbox.height)
        .fold(f32::NEG_INFINITY, f32::max);

    PdfRect::new(min_x, min_y, max_x - min_x, max_y - min_y)
}

fn median_glyph_center_y(line: &[RawGlyph]) -> f32 {
    median(line.iter().map(|glyph| rect_center_y(glyph.bbox)).collect())
}

fn median_glyph_height(line: &[RawGlyph]) -> f32 {
    median(line.iter().map(|glyph| glyph.bbox.height).collect())
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

fn vertical_overlap_ratio(left: PdfRect, right: PdfRect) -> f32 {
    let overlap = (left.y + left.height).min(right.y + right.height) - left.y.max(right.y);
    if overlap <= 0.0 {
        return 0.0;
    }

    overlap / left.height.min(right.height).max(1.0)
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

fn min_x(line: &[RawGlyph]) -> f32 {
    line.iter()
        .map(|glyph| glyph.bbox.x)
        .fold(f32::INFINITY, f32::min)
}

fn source_span(line: &[RawGlyph]) -> (usize, usize) {
    line.iter()
        .map(|glyph| glyph.source_index)
        .fold((usize::MAX, 0), |(min, max), source_index| {
            (min.min(source_index), max.max(source_index))
        })
}

fn source_gap(left: (usize, usize), right: (usize, usize)) -> usize {
    if left.1 < right.0 {
        right.0 - left.1
    } else if right.1 < left.0 {
        left.0.saturating_sub(right.1)
    } else {
        0
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
