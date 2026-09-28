use termpdf::document::{Document, Page};

pub fn sample_document() -> Document {
    Document {
        outline: Vec::new(),
        pages: vec![
            Page::from_text(0, &["alpha beta", "beta gamma", "zeta"]),
            Page::from_text(1, &["beta delta", "omega beta"]),
        ],
    }
}
