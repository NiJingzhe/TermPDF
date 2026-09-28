use std::error::Error;
use std::fmt;
use std::str::FromStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentRef {
    Page {
        page: usize,
    },
    TextLine {
        page: usize,
        line: usize,
    },
    Glyph {
        page: usize,
        line: usize,
        glyph: usize,
    },
    Link {
        page: usize,
        link: usize,
    },
    Image {
        page: usize,
        image: usize,
    },
}

impl DocumentRef {
    pub const fn page_index(self) -> usize {
        match self {
            Self::Page { page }
            | Self::TextLine { page, .. }
            | Self::Glyph { page, .. }
            | Self::Link { page, .. }
            | Self::Image { page, .. } => page,
        }
    }
}

impl fmt::Display for DocumentRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Page { page } => write!(formatter, "p{}", page + 1),
            Self::TextLine { page, line } => write!(formatter, "p{}.t{}", page + 1, line + 1),
            Self::Glyph { page, line, glyph } => {
                write!(formatter, "p{}.t{}.c{}", page + 1, line + 1, glyph + 1)
            }
            Self::Link { page, link } => write!(formatter, "p{}.link{}", page + 1, link + 1),
            Self::Image { page, image } => {
                write!(formatter, "p{}.image{}", page + 1, image + 1)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseDocumentRefError {
    input: String,
}

impl ParseDocumentRefError {
    fn new(input: &str) -> Self {
        Self {
            input: input.to_string(),
        }
    }
}

impl fmt::Display for ParseDocumentRefError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid document ref '{}'", self.input)
    }
}

impl Error for ParseDocumentRefError {}

impl FromStr for DocumentRef {
    type Err = ParseDocumentRefError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let parts = input.split('.').collect::<Vec<_>>();
        let page = parse_one_based(parts.first().copied(), "p")
            .ok_or_else(|| ParseDocumentRefError::new(input))?;

        match parts.as_slice() {
            [_] => Ok(Self::Page { page }),
            [_, target] => {
                if let Some(line) = parse_one_based(Some(target), "t") {
                    return Ok(Self::TextLine { page, line });
                }
                if let Some(link) = parse_one_based(Some(target), "link") {
                    return Ok(Self::Link { page, link });
                }
                if let Some(image) = parse_one_based(Some(target), "image") {
                    return Ok(Self::Image { page, image });
                }
                Err(ParseDocumentRefError::new(input))
            }
            [_, line, glyph] => {
                let line = parse_one_based(Some(line), "t")
                    .ok_or_else(|| ParseDocumentRefError::new(input))?;
                let glyph = parse_one_based(Some(glyph), "c")
                    .ok_or_else(|| ParseDocumentRefError::new(input))?;
                Ok(Self::Glyph { page, line, glyph })
            }
            _ => Err(ParseDocumentRefError::new(input)),
        }
    }
}

fn parse_one_based(component: Option<&str>, prefix: &str) -> Option<usize> {
    let digits = component?.strip_prefix(prefix)?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse::<usize>().ok()?.checked_sub(1)
}
