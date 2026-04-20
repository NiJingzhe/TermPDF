#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PdfRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl PdfRect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Glyph {
    pub ch: char,
    pub bbox: PdfRect,
    pub page: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PdfLine {
    pub glyphs: Vec<Glyph>,
    pub bbox: PdfRect,
}

impl PdfLine {
    pub fn text(&self) -> String {
        self.glyphs.iter().map(|glyph| glyph.ch).collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum LinkTarget {
    LocalDestination {
        page: usize,
        x: Option<f32>,
        y: Option<f32>,
        zoom: Option<f32>,
    },
    ExternalUri(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PageLink {
    pub bbox: PdfRect,
    pub target: LinkTarget,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub lines: Vec<PdfLine>,
    pub bbox: PdfRect,
    pub links: Vec<PageLink>,
}

impl Page {
    pub fn from_text(page_number: usize, lines: &[&str]) -> Self {
        let line_height = 18.0;
        let mut built_lines = Vec::with_capacity(lines.len());

        for (line_index, text) in lines.iter().enumerate() {
            let y = line_index as f32 * line_height;
            let glyphs = text
                .chars()
                .enumerate()
                .map(|(column, ch)| Glyph {
                    ch,
                    bbox: PdfRect::new(column as f32 * 9.0, y, 9.0, line_height),
                    page: page_number,
                })
                .collect::<Vec<_>>();
            let width = glyphs
                .last()
                .map(|glyph| glyph.bbox.x + glyph.bbox.width)
                .unwrap_or(0.0);

            built_lines.push(PdfLine {
                glyphs,
                bbox: PdfRect::new(0.0, y, width, line_height),
            });
        }

        let height = built_lines.len() as f32 * line_height;
        Self {
            lines: built_lines,
            bbox: PdfRect::new(0.0, 0.0, 595.0, height.max(line_height)),
            links: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub pages: Vec<Page>,
}

impl Document {
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    pub fn lines(&self) -> impl Iterator<Item = (usize, usize, &PdfLine)> {
        self.pages
            .iter()
            .enumerate()
            .flat_map(|(page_index, page)| {
                page.lines
                    .iter()
                    .enumerate()
                    .map(move |(line_index, line)| (page_index, line_index, line))
            })
    }
}
