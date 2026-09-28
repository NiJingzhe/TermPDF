use std::collections::BTreeMap;
use std::fs;

use termpdf::marks::{MarkStore, NamedMark};

#[test]
fn rejects_malformed_and_unsupported_marks_files() {
    let temp = tempfile::tempdir().unwrap();
    let pdf_path = temp.path().join("paper.pdf");
    let marks_path = temp.path().join("marks.json");
    fs::write(&pdf_path, b"PDF contents").unwrap();

    fs::write(&marks_path, b"not json").unwrap();
    assert!(MarkStore::open(&pdf_path, marks_path.clone()).is_err());

    fs::write(
        &marks_path,
        br#"{"schema":"termpdf.marks.v999","documents":{}}"#,
    )
    .unwrap();
    let error = MarkStore::open(&pdf_path, marks_path).unwrap_err();
    assert!(error.to_string().contains("unsupported marks schema"));
}

#[test]
fn stores_multiple_documents_without_overwriting_existing_marks() {
    let temp = tempfile::tempdir().unwrap();
    let first_pdf = temp.path().join("first.pdf");
    let second_pdf = temp.path().join("second.pdf");
    let marks_path = temp.path().join("marks.json");
    fs::write(&first_pdf, b"first PDF").unwrap();
    fs::write(&second_pdf, b"second PDF").unwrap();

    let (mut first_store, _) = MarkStore::open(&first_pdf, marks_path.clone()).unwrap();
    first_store
        .save(&BTreeMap::from([("first".to_string(), mark("p1"))]))
        .unwrap();
    let (mut second_store, _) = MarkStore::open(&second_pdf, marks_path.clone()).unwrap();
    second_store
        .save(&BTreeMap::from([("second".to_string(), mark("p2"))]))
        .unwrap();

    let (_, first_marks) = MarkStore::open(&first_pdf, marks_path.clone()).unwrap();
    let (_, second_marks) = MarkStore::open(&second_pdf, marks_path).unwrap();
    assert_eq!(first_marks.keys().collect::<Vec<_>>(), ["first"]);
    assert_eq!(second_marks.keys().collect::<Vec<_>>(), ["second"]);
}

fn mark(ref_id: &str) -> NamedMark {
    NamedMark {
        ref_id: ref_id.to_string(),
        page_index: 0,
        relative_x: 0.5,
        relative_y: 0.5,
        zoom_percent: 100,
    }
}
