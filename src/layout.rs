use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, WrapErr, bail};
use regex::RegexBuilder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::document::{Document, LinkTarget, PageLink, PdfRect};

pub const LAYOUT_SCHEMA: &str = "termpdf.layout.v1";
pub const MANIFEST_FILE: &str = "manifest.json";
pub const PAGES_FILE: &str = "pages.jsonl";
pub const BLOCKS_FILE: &str = "blocks.jsonl";
pub const GLYPHS_FILE: &str = "glyphs.jsonl";
pub const REFS_FILE: &str = "refs.jsonl";

const TEXT_PREVIEW_CHARS: usize = 80;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutWriteOptions {
    pub overwrite: bool,
}

impl LayoutWriteOptions {
    pub const fn new(overwrite: bool) -> Self {
        Self { overwrite }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutWriteResult {
    pub output_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceMetadata {
    pub file_name: Option<String>,
    pub sha256: String,
    pub size_bytes: u64,
}

impl SourceMetadata {
    pub fn from_path(path: &Path) -> Result<Self> {
        let metadata = fs::metadata(path)
            .wrap_err_with(|| format!("failed to stat source PDF {}", path.display()))?;
        let sha256 = sha256_file(path)?;
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());

        Ok(Self {
            file_name,
            sha256,
            size_bytes: metadata.len(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LayoutManifest {
    pub schema: &'static str,
    pub termpdf_version: &'static str,
    pub source: LayoutSource,
    pub coordinate_system: CoordinateSystem,
    pub files: LayoutFiles,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LayoutSource {
    pub file_name: Option<String>,
    pub sha256: String,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CoordinateSystem {
    pub unit: &'static str,
    pub origin: &'static str,
    pub x_axis: &'static str,
    pub y_axis: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LayoutFiles {
    pub pages: &'static str,
    pub blocks: &'static str,
    pub glyphs: &'static str,
    pub refs: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub struct LayoutRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl From<PdfRect> for LayoutRect {
    fn from(rect: PdfRect) -> Self {
        Self {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct LayoutPage {
    #[serde(rename = "ref")]
    pub ref_id: String,
    pub kind: LayoutKind,
    pub index: usize,
    pub number: usize,
    pub bbox: LayoutRect,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind")]
pub enum LayoutBlock {
    #[serde(rename = "text_line")]
    TextLine {
        #[serde(rename = "ref")]
        ref_id: String,
        page_ref: String,
        page_index: usize,
        reading_order: usize,
        bbox: LayoutRect,
        text: String,
        glyph_refs: Vec<String>,
    },
    #[serde(rename = "link")]
    Link {
        #[serde(rename = "ref")]
        ref_id: String,
        page_ref: String,
        page_index: usize,
        reading_order: usize,
        bbox: LayoutRect,
        target: LayoutLinkTarget,
    },
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct LayoutGlyph {
    #[serde(rename = "ref")]
    pub ref_id: String,
    pub kind: LayoutKind,
    pub page_ref: String,
    pub parent_ref: String,
    pub page_index: usize,
    pub line_index: usize,
    pub char_index: usize,
    #[serde(rename = "char")]
    pub ch: char,
    pub bbox: LayoutRect,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind")]
pub enum LayoutLinkTarget {
    #[serde(rename = "external_uri")]
    ExternalUri { uri: String },
    #[serde(rename = "local_destination")]
    LocalDestination {
        page_index: usize,
        page_ref: String,
        x: Option<f32>,
        y: Option<f32>,
        zoom: Option<f32>,
    },
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct LayoutRef {
    #[serde(rename = "ref")]
    pub ref_id: String,
    pub kind: LayoutKind,
    pub page_ref: Option<String>,
    pub bbox: Option<LayoutRect>,
    pub text_preview: Option<String>,
    pub target_file: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutKind {
    Page,
    TextLine,
    Glyph,
    Link,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutPack {
    pub manifest: LayoutManifest,
    pub pages: Vec<LayoutPage>,
    pub blocks: Vec<LayoutBlock>,
    pub glyphs: Vec<LayoutGlyph>,
    pub refs: Vec<LayoutRef>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutGrepOptions {
    pub ignore_case: bool,
    pub regex_mode: bool,
}

impl LayoutGrepOptions {
    pub const fn new(ignore_case: bool, regex_mode: bool) -> Self {
        Self {
            ignore_case,
            regex_mode,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LayoutGrepMatch {
    #[serde(rename = "ref")]
    pub ref_id: String,
    pub page_ref: String,
    pub page_index: usize,
    pub bbox: LayoutRect,
    pub text: String,
    pub matches: Vec<LayoutGrepRange>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LayoutGrepRange {
    pub byte_start: usize,
    pub byte_end: usize,
    pub char_start: usize,
    pub char_end: usize,
}

impl LayoutPack {
    pub fn from_document(document: &Document, source: SourceMetadata) -> Self {
        let manifest = LayoutManifest {
            schema: LAYOUT_SCHEMA,
            termpdf_version: env!("CARGO_PKG_VERSION"),
            source: LayoutSource {
                file_name: source.file_name,
                sha256: source.sha256,
                size_bytes: source.size_bytes,
            },
            coordinate_system: CoordinateSystem {
                unit: "pdf_point",
                origin: "bottom_left",
                x_axis: "right",
                y_axis: "up",
            },
            files: LayoutFiles {
                pages: PAGES_FILE,
                blocks: BLOCKS_FILE,
                glyphs: GLYPHS_FILE,
                refs: REFS_FILE,
            },
        };

        let mut pages = Vec::with_capacity(document.pages.len());
        let mut blocks = Vec::new();
        let mut glyphs = Vec::new();
        let mut refs = Vec::new();

        for (page_index, page) in document.pages.iter().enumerate() {
            let page_ref = page_ref(page_index);
            let page_bbox = LayoutRect::from(page.bbox);
            pages.push(LayoutPage {
                ref_id: page_ref.clone(),
                kind: LayoutKind::Page,
                index: page_index,
                number: page_index + 1,
                bbox: page_bbox,
            });
            refs.push(LayoutRef {
                ref_id: page_ref.clone(),
                kind: LayoutKind::Page,
                page_ref: Some(page_ref.clone()),
                bbox: Some(page_bbox),
                text_preview: None,
                target_file: PAGES_FILE,
            });

            for (line_index, line) in page.lines.iter().enumerate() {
                let text_ref = text_line_ref(page_index, line_index);
                let glyph_refs = (0..line.glyphs.len())
                    .map(|glyph_index| glyph_ref(page_index, line_index, glyph_index))
                    .collect::<Vec<_>>();
                let text = line.text();
                let line_bbox = LayoutRect::from(line.bbox);

                blocks.push(LayoutBlock::TextLine {
                    ref_id: text_ref.clone(),
                    page_ref: page_ref.clone(),
                    page_index,
                    reading_order: line_index + 1,
                    bbox: line_bbox,
                    text: text.clone(),
                    glyph_refs,
                });
                refs.push(LayoutRef {
                    ref_id: text_ref.clone(),
                    kind: LayoutKind::TextLine,
                    page_ref: Some(page_ref.clone()),
                    bbox: Some(line_bbox),
                    text_preview: Some(text_preview(&text)),
                    target_file: BLOCKS_FILE,
                });

                for (glyph_index, glyph) in line.glyphs.iter().enumerate() {
                    let glyph_ref = glyph_ref(page_index, line_index, glyph_index);
                    let glyph_bbox = LayoutRect::from(glyph.bbox);
                    glyphs.push(LayoutGlyph {
                        ref_id: glyph_ref.clone(),
                        kind: LayoutKind::Glyph,
                        page_ref: page_ref.clone(),
                        parent_ref: text_ref.clone(),
                        page_index,
                        line_index,
                        char_index: glyph_index,
                        ch: glyph.ch,
                        bbox: glyph_bbox,
                    });
                    refs.push(LayoutRef {
                        ref_id: glyph_ref,
                        kind: LayoutKind::Glyph,
                        page_ref: Some(page_ref.clone()),
                        bbox: Some(glyph_bbox),
                        text_preview: Some(glyph.ch.to_string()),
                        target_file: GLYPHS_FILE,
                    });
                }
            }

            for (link_index, link) in sorted_links(&page.links).into_iter().enumerate() {
                let link_ref = link_ref(page_index, link_index);
                let link_bbox = LayoutRect::from(link.bbox);
                blocks.push(LayoutBlock::Link {
                    ref_id: link_ref.clone(),
                    page_ref: page_ref.clone(),
                    page_index,
                    reading_order: link_index + 1,
                    bbox: link_bbox,
                    target: LayoutLinkTarget::from_link_target(&link.target),
                });
                refs.push(LayoutRef {
                    ref_id: link_ref,
                    kind: LayoutKind::Link,
                    page_ref: Some(page_ref.clone()),
                    bbox: Some(link_bbox),
                    text_preview: None,
                    target_file: BLOCKS_FILE,
                });
            }
        }

        Self {
            manifest,
            pages,
            blocks,
            glyphs,
            refs,
        }
    }

    pub fn write_to_dir(
        &self,
        output_dir: &Path,
        options: LayoutWriteOptions,
    ) -> Result<LayoutWriteResult> {
        validate_output_dir(output_dir, options.overwrite)?;
        let staging_dir = create_staging_dir(output_dir)?;
        let write_result = (|| -> Result<()> {
            write_json_pretty(staging_dir.join(MANIFEST_FILE), &self.manifest)?;
            write_jsonl(staging_dir.join(PAGES_FILE), &self.pages)?;
            write_jsonl(staging_dir.join(BLOCKS_FILE), &self.blocks)?;
            write_jsonl(staging_dir.join(GLYPHS_FILE), &self.glyphs)?;
            write_jsonl(staging_dir.join(REFS_FILE), &self.refs)?;
            Ok(())
        })();

        if let Err(error) = write_result {
            let _ = fs::remove_dir_all(&staging_dir);
            return Err(error);
        }

        replace_output_dir(output_dir, &staging_dir, options.overwrite)?;

        Ok(LayoutWriteResult {
            output_dir: output_dir.to_path_buf(),
        })
    }
}

pub fn grep_layout_pack(
    layout_dir: &Path,
    pattern: &str,
    options: LayoutGrepOptions,
) -> Result<Vec<LayoutGrepMatch>> {
    if pattern.is_empty() {
        bail!("grep pattern must not be empty");
    }

    ensure_layout_pack_dir(layout_dir)?;
    let pattern = if options.regex_mode {
        pattern.to_string()
    } else {
        regex::escape(pattern)
    };
    let regex = RegexBuilder::new(&pattern)
        .case_insensitive(options.ignore_case)
        .build()
        .wrap_err("failed to compile grep pattern")?;
    let blocks = read_jsonl::<LayoutBlock>(&layout_dir.join(BLOCKS_FILE))?;
    let mut found = Vec::new();

    for block in blocks {
        let LayoutBlock::TextLine {
            ref_id,
            page_ref,
            page_index,
            bbox,
            text,
            ..
        } = block
        else {
            continue;
        };
        let matches = regex
            .find_iter(&text)
            .map(|matched| LayoutGrepRange {
                byte_start: matched.start(),
                byte_end: matched.end(),
                char_start: byte_to_char_index(&text, matched.start()),
                char_end: byte_to_char_index(&text, matched.end()),
            })
            .collect::<Vec<_>>();

        if !matches.is_empty() {
            found.push(LayoutGrepMatch {
                ref_id,
                page_ref,
                page_index,
                bbox,
                text,
                matches,
            });
        }
    }

    Ok(found)
}

impl LayoutLinkTarget {
    fn from_link_target(target: &LinkTarget) -> Self {
        match target {
            LinkTarget::ExternalUri(uri) => Self::ExternalUri { uri: uri.clone() },
            LinkTarget::LocalDestination { page, x, y, zoom } => Self::LocalDestination {
                page_index: *page,
                page_ref: page_ref(*page),
                x: *x,
                y: *y,
                zoom: *zoom,
            },
        }
    }
}

pub fn default_layout_output_dir(pdf_path: &Path) -> PathBuf {
    let file_name = pdf_path.file_name().filter(|name| !name.is_empty());
    let layout_name = if pdf_path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false)
    {
        let stem = pdf_path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .filter(|stem| !stem.is_empty())
            .unwrap_or_else(|| "document".to_string());
        format!("{stem}.layout")
    } else {
        let file_name = file_name
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "document".to_string());
        format!("{file_name}.layout")
    };

    if let Some(parent) = pdf_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        parent.join(layout_name)
    } else {
        PathBuf::from(layout_name)
    }
}

pub fn page_ref(page_index: usize) -> String {
    format!("p{}", page_index + 1)
}

pub fn text_line_ref(page_index: usize, line_index: usize) -> String {
    format!("{}.t{}", page_ref(page_index), line_index + 1)
}

pub fn glyph_ref(page_index: usize, line_index: usize, glyph_index: usize) -> String {
    format!(
        "{}.c{}",
        text_line_ref(page_index, line_index),
        glyph_index + 1
    )
}

pub fn link_ref(page_index: usize, link_index: usize) -> String {
    format!("{}.link{}", page_ref(page_index), link_index + 1)
}

fn text_preview(text: &str) -> String {
    let mut preview = text.chars().take(TEXT_PREVIEW_CHARS).collect::<String>();
    if text.chars().count() > TEXT_PREVIEW_CHARS {
        preview.push_str("...");
    }
    preview
}

fn sorted_links(links: &[PageLink]) -> Vec<&PageLink> {
    let mut sorted = links.iter().collect::<Vec<_>>();
    sorted.sort_by(|left, right| {
        let left_top = left.bbox.y + left.bbox.height;
        let right_top = right.bbox.y + right.bbox.height;
        right_top
            .partial_cmp(&left_top)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                left.bbox
                    .x
                    .partial_cmp(&right.bbox.x)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    sorted
}

fn validate_output_dir(output_dir: &Path, overwrite: bool) -> Result<()> {
    if !output_dir.exists() {
        return Ok(());
    }

    let output_type = fs::symlink_metadata(output_dir)
        .wrap_err_with(|| format!("failed to inspect output path {}", output_dir.display()))?
        .file_type();
    if output_type.is_symlink() {
        bail!(
            "refusing to write layout pack through symlink: {}",
            output_dir.display()
        );
    }

    if !output_dir.is_dir() {
        bail!(
            "output path exists and is not a directory: {}",
            output_dir.display()
        );
    }

    let entries = fs::read_dir(output_dir)
        .wrap_err_with(|| format!("failed to read output directory {}", output_dir.display()))?
        .collect::<std::io::Result<Vec<_>>>()
        .wrap_err_with(|| format!("failed to read output directory {}", output_dir.display()))?;

    if entries.is_empty() {
        return Ok(());
    }

    if !is_layout_pack_dir(output_dir, &entries)? {
        bail!(
            "output directory is not an existing TermPDF layout pack: {}",
            output_dir.display()
        );
    }

    if !overwrite {
        bail!(
            "output directory already contains a TermPDF layout pack; pass --overwrite to replace it: {}",
            output_dir.display()
        );
    }

    Ok(())
}

fn ensure_layout_pack_dir(layout_dir: &Path) -> Result<()> {
    if !layout_dir.exists() {
        bail!(
            "layout pack directory does not exist: {}",
            layout_dir.display()
        );
    }
    if !layout_dir.is_dir() {
        bail!(
            "layout pack path is not a directory: {}",
            layout_dir.display()
        );
    }

    let manifest_path = layout_dir.join(MANIFEST_FILE);
    let manifest = fs::read_to_string(&manifest_path)
        .wrap_err_with(|| format!("failed to read {}", manifest_path.display()))?;
    let manifest = serde_json::from_str::<serde_json::Value>(&manifest)
        .wrap_err_with(|| format!("failed to parse {}", manifest_path.display()))?;
    if manifest.get("schema").and_then(|value| value.as_str()) != Some(LAYOUT_SCHEMA) {
        bail!(
            "unsupported layout pack schema in {}",
            manifest_path.display()
        );
    }

    Ok(())
}

fn create_staging_dir(output_dir: &Path) -> Result<PathBuf> {
    create_unique_sibling_dir(output_dir, "tmp")
}

fn create_unique_sibling_dir(output_dir: &Path, suffix: &str) -> Result<PathBuf> {
    let parent = output_dir
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .wrap_err_with(|| format!("failed to create parent directory {}", parent.display()))?;

    let base_name = output_dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "layout".to_string());
    for attempt in 0..1000 {
        let candidate = parent.join(format!(
            ".{base_name}.{suffix}-{}-{attempt}",
            std::process::id()
        ));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).wrap_err_with(|| {
                    format!("failed to create staging directory {}", candidate.display())
                });
            }
        }
    }

    bail!(
        "failed to create a unique {suffix} directory next to {}",
        output_dir.display()
    )
}

fn replace_output_dir(output_dir: &Path, staging_dir: &Path, overwrite: bool) -> Result<()> {
    validate_output_dir(output_dir, overwrite)?;

    if !output_dir.exists() {
        return rename_staging_dir(output_dir, staging_dir);
    }

    let backup_dir = create_unique_sibling_dir(output_dir, "backup")?;
    fs::remove_dir(&backup_dir)
        .wrap_err_with(|| format!("failed to reserve backup path {}", backup_dir.display()))?;
    fs::rename(output_dir, &backup_dir).wrap_err_with(|| {
        format!(
            "failed to move existing layout pack {} to backup {}",
            output_dir.display(),
            backup_dir.display()
        )
    })?;

    match rename_staging_dir(output_dir, staging_dir) {
        Ok(()) => {
            let _ = fs::remove_dir_all(&backup_dir);
            Ok(())
        }
        Err(error) => {
            let _ = fs::rename(&backup_dir, output_dir);
            let _ = fs::remove_dir_all(staging_dir);
            Err(error)
        }
    }
}

fn rename_staging_dir(output_dir: &Path, staging_dir: &Path) -> Result<()> {
    match fs::rename(staging_dir, output_dir) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_dir_all(staging_dir);
            Err(error).wrap_err_with(|| {
                format!("failed to move layout pack into {}", output_dir.display())
            })
        }
    }
}

fn is_layout_pack_dir(output_dir: &Path, entries: &[fs::DirEntry]) -> Result<bool> {
    let allowed = layout_file_names();
    for entry in entries {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return Ok(false);
        };
        if !allowed.contains(name) {
            return Ok(false);
        }
        let file_type = entry
            .file_type()
            .wrap_err_with(|| format!("failed to inspect {}", entry.path().display()))?;
        if !file_type.is_file() {
            return Ok(false);
        }
    }

    let manifest_path = output_dir.join(MANIFEST_FILE);
    if !manifest_path.exists() {
        return Ok(false);
    }

    let manifest = fs::read_to_string(&manifest_path)
        .wrap_err_with(|| format!("failed to read {}", manifest_path.display()))?;
    let manifest = serde_json::from_str::<serde_json::Value>(&manifest)
        .wrap_err_with(|| format!("failed to parse {}", manifest_path.display()))?;
    Ok(manifest.get("schema").and_then(|value| value.as_str()) == Some(LAYOUT_SCHEMA))
}

fn layout_file_names() -> HashSet<&'static str> {
    [
        MANIFEST_FILE,
        PAGES_FILE,
        BLOCKS_FILE,
        GLYPHS_FILE,
        REFS_FILE,
    ]
    .into_iter()
    .collect()
}

fn write_json_pretty<T: Serialize>(path: PathBuf, value: &T) -> Result<()> {
    let file =
        File::create(&path).wrap_err_with(|| format!("failed to create {}", path.display()))?;
    let writer = BufWriter::new(file);
    serde_json::to_writer_pretty(writer, value)
        .wrap_err_with(|| format!("failed to write JSON {}", path.display()))?;
    append_newline(&path)
}

fn write_jsonl<T: Serialize>(path: PathBuf, values: &[T]) -> Result<()> {
    let file =
        File::create(&path).wrap_err_with(|| format!("failed to create {}", path.display()))?;
    let mut writer = BufWriter::new(file);
    for value in values {
        serde_json::to_writer(&mut writer, value)
            .wrap_err_with(|| format!("failed to write JSONL {}", path.display()))?;
        writer
            .write_all(b"\n")
            .wrap_err_with(|| format!("failed to write JSONL {}", path.display()))?;
    }
    writer
        .flush()
        .wrap_err_with(|| format!("failed to flush {}", path.display()))
}

fn read_jsonl<T>(path: &Path) -> Result<Vec<T>>
where
    T: for<'de> Deserialize<'de>,
{
    let content = fs::read_to_string(path)
        .wrap_err_with(|| format!("failed to read JSONL {}", path.display()))?;
    content
        .lines()
        .enumerate()
        .map(|(line_index, line)| {
            serde_json::from_str(line).wrap_err_with(|| {
                format!(
                    "failed to parse JSONL {} line {}",
                    path.display(),
                    line_index + 1
                )
            })
        })
        .collect()
}

fn byte_to_char_index(text: &str, byte_index: usize) -> usize {
    text[..byte_index].chars().count()
}

fn append_newline(path: &Path) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(path)
        .wrap_err_with(|| format!("failed to open {}", path.display()))?;
    file.write_all(b"\n")
        .wrap_err_with(|| format!("failed to finish {}", path.display()))
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file =
        File::open(path).wrap_err_with(|| format!("failed to open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];

    loop {
        let read = file
            .read(&mut buffer)
            .wrap_err_with(|| format!("failed to read {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}
