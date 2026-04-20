use crate::document::{Document, PdfRect};

#[derive(Clone, Debug, PartialEq)]
pub struct SearchMatch {
    pub start: usize,
    pub end: usize,
    pub page: usize,
    pub line: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IndexedChar {
    pub ch: char,
    pub page: usize,
    pub line: usize,
    pub glyph: usize,
    pub bbox: PdfRect,
}

#[derive(Clone, Debug)]
pub struct DocumentIndex {
    pub full_text: String,
    pub chars: Vec<IndexedChar>,
    byte_offsets: Vec<usize>,
}

impl DocumentIndex {
    pub fn build(document: &Document) -> Self {
        let mut full_text = String::new();
        let mut chars = Vec::new();
        let mut byte_offsets = Vec::new();

        for (page_index, line_index, line) in document.lines() {
            for (glyph_index, glyph) in line.glyphs.iter().enumerate() {
                byte_offsets.push(full_text.len());
                full_text.push(glyph.ch);
                chars.push(IndexedChar {
                    ch: glyph.ch,
                    page: page_index,
                    line: line_index,
                    glyph: glyph_index,
                    bbox: glyph.bbox,
                });
            }

            byte_offsets.push(full_text.len());
            full_text.push('\n');
            chars.push(IndexedChar {
                ch: '\n',
                page: page_index,
                line: line_index,
                glyph: line.glyphs.len(),
                bbox: line.bbox,
            });
        }

        byte_offsets.push(full_text.len());

        Self {
            full_text,
            chars,
            byte_offsets,
        }
    }

    pub fn search(&self, query: &str) -> Vec<SearchMatch> {
        if query.is_empty() {
            return Vec::new();
        }

        self.full_text
            .match_indices(query)
            .filter_map(|(start, matched)| {
                let end = start + matched.len();
                let start_index = self.byte_offsets.binary_search(&start).ok()?;
                let end_index = self.byte_offsets.binary_search(&end).ok()?;
                let indexed = self.chars.get(start_index)?;
                Some(SearchMatch {
                    start: start_index,
                    end: end_index,
                    page: indexed.page,
                    line: indexed.line,
                })
            })
            .collect()
    }

    pub fn selection_for_match(&self, search_match: &SearchMatch) -> Vec<IndexedChar> {
        self.chars[search_match.start..search_match.end].to_vec()
    }

    pub fn selection_bounds_for_match(&self, search_match: &SearchMatch) -> Option<PdfRect> {
        let glyphs = self.chars.get(search_match.start..search_match.end)?;
        let first = glyphs.first()?;

        let mut min_x = first.bbox.x;
        let mut min_y = first.bbox.y;
        let mut max_x = first.bbox.x + first.bbox.width;
        let mut max_y = first.bbox.y + first.bbox.height;

        for glyph in glyphs.iter().skip(1) {
            min_x = min_x.min(glyph.bbox.x);
            min_y = min_y.min(glyph.bbox.y);
            max_x = max_x.max(glyph.bbox.x + glyph.bbox.width);
            max_y = max_y.max(glyph.bbox.y + glyph.bbox.height);
        }

        Some(PdfRect::new(min_x, min_y, max_x - min_x, max_y - min_y))
    }

    pub fn selection_bounds_for_page_matches(
        &self,
        matches: &[SearchMatch],
        page: usize,
    ) -> Vec<PdfRect> {
        matches
            .iter()
            .filter(|matched| matched.page == page)
            .filter_map(|matched| self.selection_bounds_for_match(matched))
            .collect()
    }
}
