use std::cmp::Ordering;
use std::env;
use std::path::{Path, PathBuf};

use clap::Parser;
use color_eyre::eyre::{bail, OptionExt, Result, WrapErr};
use pdfium_render::prelude::*;

use crate::document::{Document, Glyph, LinkTarget, Page, PageLink, PdfLine, PdfRect};
use crate::pdfium_bundle::{bundled_pdfium_vendor_dir, packaged_pdfium_library_name};
use crate::render::{PageRenderCache, PageRenderPlan, RenderedPage};

const LINE_MERGE_TOLERANCE_FACTOR: f32 = 0.6;

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
    after_help = "Keybindings:\n  hjkl                Pan viewport\n  Ctrl-u / Ctrl-d     Half-page up/down\n  Ctrl-b / Ctrl-f     Full-page back/forward\n  gg / {count}gg / G  Jump to page\n  /, n, N, Esc        Search, navigate, hide highlight\n  f / F               Follow visible links\n  m<char> / `<char>   Set and jump to marks\n  F5                  Presentation mode\n  = / - / 0           Zoom in / out / reset\n  i                   Toggle dark mode\n  q                   Quit"
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
            if arg == PathBuf::from("-w") || arg == PathBuf::from("--watch") {
                watch_mode = true;
                continue;
            }

            if arg == PathBuf::from("--pdfium-lib") {
                let value = args.next().ok_or_eyre("missing value for --pdfium-lib")?;
                pdfium_lib_path = Some(value);
                continue;
            }

            if arg == PathBuf::from("--dark") {
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
    let vendor_dir = bundled_pdfium_vendor_dir(os, arch)?;
    let library_name = packaged_pdfium_library_name(os)?;
    Some(
        project_root
            .join("vendor/pdfium")
            .join(vendor_dir)
            .join("lib")
            .join(library_name),
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
        });
    }

    Ok(Document { pages })
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

    lines
        .into_iter()
        .map(|mut line| {
            line.sort_by(|left, right| {
                left.bbox
                    .x
                    .partial_cmp(&right.bbox.x)
                    .unwrap_or(Ordering::Equal)
            });

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

            let glyphs = line
                .into_iter()
                .map(|glyph| Glyph {
                    ch: glyph.ch,
                    bbox: glyph.bbox,
                    page: glyph.page,
                })
                .collect::<Vec<_>>();

            PdfLine {
                glyphs,
                bbox: PdfRect::new(min_x, min_y, max_x - min_x, max_y - min_y),
            }
        })
        .collect()
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
    let avg_y = line.iter().map(|item| item.bbox.y).sum::<f32>() / line.len() as f32;
    let avg_height = line.iter().map(|item| item.bbox.height).sum::<f32>() / line.len() as f32;
    let tolerance = (avg_height * LINE_MERGE_TOLERANCE_FACTOR).max(1.0);

    (glyph.bbox.y - avg_y).abs() <= tolerance
}
