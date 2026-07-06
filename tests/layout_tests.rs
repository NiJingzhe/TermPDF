use std::fs;
use std::path::PathBuf;

use serde_json::Value;
use termpdf::document::{Document, LinkTarget, Page, PageLink, PdfRect};
use termpdf::layout::{
    BLOCKS_FILE, GLYPHS_FILE, LAYOUT_SCHEMA, LayoutBlock, LayoutKind, LayoutLinkTarget, LayoutPack,
    LayoutWriteOptions, MANIFEST_FILE, PAGES_FILE, REFS_FILE, SourceMetadata,
    default_layout_output_dir, glyph_ref, grep_layout_pack, link_ref, page_ref, text_line_ref,
};

fn source_metadata() -> SourceMetadata {
    SourceMetadata {
        file_name: Some("sample.pdf".to_string()),
        sha256: "00ff".to_string(),
        size_bytes: 42,
    }
}

fn document_with_links() -> Document {
    let mut page = Page::from_text(0, &["alpha beta", "你好 gamma"]);
    page.links = vec![
        PageLink {
            bbox: PdfRect::new(200.0, 10.0, 30.0, 10.0),
            target: LinkTarget::ExternalUri("https://bottom.example".to_string()),
        },
        PageLink {
            bbox: PdfRect::new(20.0, 80.0, 30.0, 10.0),
            target: LinkTarget::LocalDestination {
                page: 0,
                x: Some(10.0),
                y: Some(20.0),
                zoom: Some(1.5),
            },
        },
        PageLink {
            bbox: PdfRect::new(10.0, 80.0, 30.0, 10.0),
            target: LinkTarget::ExternalUri("https://top-left.example".to_string()),
        },
    ];

    Document { pages: vec![page] }
}

#[test]
fn stable_ref_helpers_are_one_based_and_namespaced() {
    assert_eq!(page_ref(0), "p1");
    assert_eq!(text_line_ref(0, 1), "p1.t2");
    assert_eq!(glyph_ref(0, 1, 2), "p1.t2.c3");
    assert_eq!(link_ref(0, 2), "p1.link3");
}

#[test]
fn default_layout_output_dir_replaces_pdf_extension() {
    assert_eq!(
        default_layout_output_dir("paper.pdf".as_ref()),
        PathBuf::from("paper.layout")
    );
    assert_eq!(
        default_layout_output_dir("paper.PDF".as_ref()),
        PathBuf::from("paper.layout")
    );
    assert_eq!(
        default_layout_output_dir("docs/paper.final.pdf".as_ref()),
        PathBuf::from("docs/paper.final.layout")
    );
    assert_eq!(
        default_layout_output_dir("paper".as_ref()),
        PathBuf::from("paper.layout")
    );
}

#[test]
fn source_metadata_hashes_file_content() {
    let temp = tempfile::tempdir().unwrap();
    let pdf = temp.path().join("sample.pdf");
    fs::write(&pdf, b"abc").unwrap();

    let metadata = SourceMetadata::from_path(&pdf).unwrap();

    assert_eq!(metadata.file_name.as_deref(), Some("sample.pdf"));
    assert_eq!(metadata.size_bytes, 3);
    assert_eq!(
        metadata.sha256,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn layout_pack_contains_manifest_pages_lines_glyphs_and_refs() {
    let pack = LayoutPack::from_document(&document_with_links(), source_metadata());

    assert_eq!(pack.manifest.schema, LAYOUT_SCHEMA);
    assert_eq!(
        pack.manifest.source.file_name.as_deref(),
        Some("sample.pdf")
    );
    assert_eq!(pack.manifest.coordinate_system.origin, "bottom_left");
    assert_eq!(pack.pages.len(), 1);
    assert_eq!(pack.pages[0].ref_id, "p1");
    assert_eq!(pack.pages[0].kind, LayoutKind::Page);
    assert_eq!(pack.pages[0].number, 1);

    let text_lines = pack
        .blocks
        .iter()
        .filter_map(|block| match block {
            LayoutBlock::TextLine {
                ref_id,
                text,
                glyph_refs,
                ..
            } => Some((ref_id.as_str(), text.as_str(), glyph_refs)),
            LayoutBlock::Link { .. } => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(text_lines.len(), 2);
    assert_eq!(text_lines[0].0, "p1.t1");
    assert_eq!(text_lines[0].1, "alpha beta");
    assert_eq!(text_lines[0].2[0], "p1.t1.c1");
    assert_eq!(text_lines[1].0, "p1.t2");
    assert_eq!(text_lines[1].1, "你好 gamma");

    assert_eq!(pack.glyphs[0].ref_id, "p1.t1.c1");
    assert_eq!(pack.glyphs[0].ch, 'a');
    assert_eq!(pack.glyphs[0].char_index, 0);
    assert_eq!(pack.glyphs[10].ref_id, "p1.t2.c1");
    assert_eq!(pack.glyphs[10].ch, '你');
    assert!(pack.refs.iter().any(|entry| entry.ref_id == "p1"));
    assert!(pack.refs.iter().any(|entry| entry.ref_id == "p1.t1"));
    assert!(pack.refs.iter().any(|entry| entry.ref_id == "p1.t1.c1"));
    assert!(pack.refs.iter().any(|entry| entry.ref_id == "p1.link1"));
}

#[test]
fn links_are_sorted_by_visual_order_and_targets_are_serialized() {
    let pack = LayoutPack::from_document(&document_with_links(), source_metadata());
    let links = pack
        .blocks
        .iter()
        .filter_map(|block| match block {
            LayoutBlock::Link { ref_id, target, .. } => Some((ref_id.as_str(), target)),
            LayoutBlock::TextLine { .. } => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(links.len(), 3);
    assert_eq!(links[0].0, "p1.link1");
    assert_eq!(
        links[0].1,
        &LayoutLinkTarget::ExternalUri {
            uri: "https://top-left.example".to_string()
        }
    );
    assert_eq!(links[1].0, "p1.link2");
    assert_eq!(
        links[1].1,
        &LayoutLinkTarget::LocalDestination {
            page_index: 0,
            page_ref: "p1".to_string(),
            x: Some(10.0),
            y: Some(20.0),
            zoom: Some(1.5),
        }
    );
    assert_eq!(links[2].0, "p1.link3");
}

#[test]
fn serialized_schema_uses_external_field_names() {
    let pack = LayoutPack::from_document(&document_with_links(), source_metadata());
    let page_value = serde_json::to_value(&pack.pages[0]).unwrap();
    let block_value = serde_json::to_value(&pack.blocks[0]).unwrap();
    let glyph_value = serde_json::to_value(&pack.glyphs[0]).unwrap();

    assert_eq!(page_value["ref"], "p1");
    assert_eq!(block_value["kind"], "text_line");
    assert_eq!(block_value["ref"], "p1.t1");
    assert_eq!(glyph_value["kind"], "glyph");
    assert_eq!(glyph_value["ref"], "p1.t1.c1");
    assert_eq!(glyph_value["char"], "a");
}

#[test]
fn empty_page_keeps_page_ref_without_blocks_or_glyphs() {
    let document = Document {
        pages: vec![Page {
            lines: Vec::new(),
            bbox: PdfRect::new(0.0, 0.0, 200.0, 300.0),
            links: Vec::new(),
        }],
    };
    let pack = LayoutPack::from_document(&document, source_metadata());

    assert_eq!(pack.pages.len(), 1);
    assert_eq!(pack.pages[0].ref_id, "p1");
    assert!(pack.blocks.is_empty());
    assert!(pack.glyphs.is_empty());
    assert_eq!(pack.refs.len(), 1);
}

#[test]
fn writer_outputs_complete_json_pack() {
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("sample.layout");
    let pack = LayoutPack::from_document(&document_with_links(), source_metadata());

    pack.write_to_dir(&output, LayoutWriteOptions::new(false))
        .unwrap();

    for file_name in [
        MANIFEST_FILE,
        PAGES_FILE,
        BLOCKS_FILE,
        GLYPHS_FILE,
        REFS_FILE,
    ] {
        assert!(output.join(file_name).exists(), "missing {file_name}");
    }

    let manifest = fs::read_to_string(output.join(MANIFEST_FILE)).unwrap();
    let manifest: Value = serde_json::from_str(&manifest).unwrap();
    assert_eq!(manifest["schema"], LAYOUT_SCHEMA);
    assert_eq!(manifest["files"]["pages"], PAGES_FILE);

    for file_name in [PAGES_FILE, BLOCKS_FILE, GLYPHS_FILE, REFS_FILE] {
        let content = fs::read_to_string(output.join(file_name)).unwrap();
        assert!(!content.is_empty(), "{file_name} should not be empty");
        for line in content.lines() {
            serde_json::from_str::<Value>(line).unwrap();
        }
    }
}

#[test]
fn grep_layout_pack_returns_matching_text_line_refs() {
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("sample.layout");
    let pack = LayoutPack::from_document(&document_with_links(), source_metadata());
    pack.write_to_dir(&output, LayoutWriteOptions::new(false))
        .unwrap();

    let matches = grep_layout_pack(
        &output,
        "gamma",
        termpdf::layout::LayoutGrepOptions::new(false, false),
    )
    .unwrap();

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].ref_id, "p1.t2");
    assert_eq!(matches[0].page_ref, "p1");
    assert_eq!(matches[0].text, "你好 gamma");
    assert_eq!(matches[0].matches[0].byte_start, 7);
    assert_eq!(matches[0].matches[0].char_start, 3);
}

#[test]
fn grep_layout_pack_defaults_to_regex_and_supports_ignore_case() {
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("sample.layout");
    let pack = LayoutPack::from_document(&document_with_links(), source_metadata());
    pack.write_to_dir(&output, LayoutWriteOptions::new(false))
        .unwrap();

    let ignore_case = grep_layout_pack(
        &output,
        "ALPHA",
        termpdf::layout::LayoutGrepOptions::new(true, false),
    )
    .unwrap();
    let regex = grep_layout_pack(
        &output,
        r"alpha\s+beta",
        termpdf::layout::LayoutGrepOptions::new(false, false),
    )
    .unwrap();

    assert_eq!(ignore_case[0].ref_id, "p1.t1");
    assert_eq!(regex[0].ref_id, "p1.t1");
}

#[test]
fn grep_layout_pack_literal_mode_escapes_regex_metacharacters() {
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("sample.layout");
    let pack = LayoutPack::from_document(&document_with_links(), source_metadata());
    pack.write_to_dir(&output, LayoutWriteOptions::new(false))
        .unwrap();

    let regex = grep_layout_pack(
        &output,
        "alpha|gamma",
        termpdf::layout::LayoutGrepOptions::new(false, false),
    )
    .unwrap();
    let literal = grep_layout_pack(
        &output,
        "alpha|gamma",
        termpdf::layout::LayoutGrepOptions::new(false, true),
    )
    .unwrap();

    assert_eq!(regex.len(), 2);
    assert!(literal.is_empty());
}

#[test]
fn grep_layout_pack_rejects_empty_or_invalid_patterns() {
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("sample.layout");
    let pack = LayoutPack::from_document(&document_with_links(), source_metadata());
    pack.write_to_dir(&output, LayoutWriteOptions::new(false))
        .unwrap();

    assert!(
        grep_layout_pack(
            &output,
            "",
            termpdf::layout::LayoutGrepOptions::new(false, false),
        )
        .is_err()
    );
    assert!(
        grep_layout_pack(
            &output,
            "[",
            termpdf::layout::LayoutGrepOptions::new(false, false),
        )
        .is_err()
    );
}

#[test]
fn grep_layout_pack_rejects_wrong_schema() {
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("sample.layout");
    fs::create_dir(&output).unwrap();
    fs::write(output.join(MANIFEST_FILE), r#"{"schema":"other"}"#).unwrap();

    assert!(
        grep_layout_pack(
            &output,
            "alpha",
            termpdf::layout::LayoutGrepOptions::new(false, false),
        )
        .is_err()
    );
}

#[test]
fn writer_requires_overwrite_for_existing_layout_pack() {
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("sample.layout");
    let pack = LayoutPack::from_document(&document_with_links(), source_metadata());

    pack.write_to_dir(&output, LayoutWriteOptions::new(false))
        .unwrap();
    assert!(
        pack.write_to_dir(&output, LayoutWriteOptions::new(false))
            .is_err()
    );
    pack.write_to_dir(&output, LayoutWriteOptions::new(true))
        .unwrap();
}

#[test]
fn writer_refuses_to_overwrite_non_layout_directory() {
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("sample.layout");
    fs::create_dir(&output).unwrap();
    fs::write(output.join("user-file.txt"), "do not delete").unwrap();
    let pack = LayoutPack::from_document(&document_with_links(), source_metadata());

    assert!(
        pack.write_to_dir(&output, LayoutWriteOptions::new(true))
            .is_err()
    );
    assert_eq!(
        fs::read_to_string(output.join("user-file.txt")).unwrap(),
        "do not delete"
    );
}

#[test]
fn writer_refuses_directory_with_wrong_manifest_schema() {
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("sample.layout");
    fs::create_dir(&output).unwrap();
    fs::write(output.join(MANIFEST_FILE), r#"{"schema":"other"}"#).unwrap();
    let pack = LayoutPack::from_document(&document_with_links(), source_metadata());

    assert!(
        pack.write_to_dir(&output, LayoutWriteOptions::new(true))
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn writer_refuses_symlink_output_directory() {
    let temp = tempfile::tempdir().unwrap();
    let real_output = temp.path().join("real.layout");
    let symlink_output = temp.path().join("symlink.layout");
    fs::create_dir(&real_output).unwrap();
    std::os::unix::fs::symlink(&real_output, &symlink_output).unwrap();
    let pack = LayoutPack::from_document(&document_with_links(), source_metadata());

    assert!(
        pack.write_to_dir(&symlink_output, LayoutWriteOptions::new(false))
            .is_err()
    );
}
