use std::path::PathBuf;

use termpdf::cli::{ExtractOptions, GrepOptions, TermpdfCommand};
use termpdf::pdf::PdfBackendOptions;

#[test]
fn parses_default_view_command() {
    let parsed = TermpdfCommand::parse_for_tests(["termpdf", "sample.pdf"], None).unwrap();

    assert_eq!(
        parsed,
        TermpdfCommand::View(PdfBackendOptions {
            pdf_path: PathBuf::from("sample.pdf"),
            pdfium_lib_path: None,
            dark_mode: false,
            watch_mode: false,
        })
    );
}

#[test]
fn parses_view_flags_and_pdfium_library() {
    let parsed = TermpdfCommand::parse_for_tests(
        [
            "termpdf",
            "sample.pdf",
            "--watch",
            "--dark",
            "--pdfium-lib",
            "/opt/pdfium",
        ],
        None,
    )
    .unwrap();

    assert_eq!(
        parsed,
        TermpdfCommand::View(PdfBackendOptions {
            pdf_path: PathBuf::from("sample.pdf"),
            pdfium_lib_path: Some(PathBuf::from("/opt/pdfium")),
            dark_mode: true,
            watch_mode: true,
        })
    );
}

#[test]
fn parses_extract_command_with_default_output_dir() {
    let parsed =
        TermpdfCommand::parse_for_tests(["termpdf", "extract", "sample.pdf"], None).unwrap();

    assert_eq!(
        parsed,
        TermpdfCommand::Extract(ExtractOptions {
            pdf_path: PathBuf::from("sample.pdf"),
            pdfium_lib_path: None,
            output_dir: PathBuf::from("sample.layout"),
            overwrite: false,
        })
    );
}

#[test]
fn parses_extract_command_with_explicit_output_overwrite_and_pdfium_library() {
    let parsed = TermpdfCommand::parse_for_tests(
        [
            "termpdf",
            "--pdfium-lib",
            "/opt/pdfium",
            "extract",
            "sample.pdf",
            "--out",
            "out.layout",
            "--overwrite",
        ],
        None,
    )
    .unwrap();

    assert_eq!(
        parsed,
        TermpdfCommand::Extract(ExtractOptions {
            pdf_path: PathBuf::from("sample.pdf"),
            pdfium_lib_path: Some(PathBuf::from("/opt/pdfium")),
            output_dir: PathBuf::from("out.layout"),
            overwrite: true,
        })
    );
}

#[test]
fn parses_extract_pdfium_library_after_subcommand() {
    let parsed = TermpdfCommand::parse_for_tests(
        [
            "termpdf",
            "extract",
            "sample.pdf",
            "--pdfium-lib",
            "/opt/pdfium",
        ],
        None,
    )
    .unwrap();

    assert_eq!(
        parsed,
        TermpdfCommand::Extract(ExtractOptions {
            pdf_path: PathBuf::from("sample.pdf"),
            pdfium_lib_path: Some(PathBuf::from("/opt/pdfium")),
            output_dir: PathBuf::from("sample.layout"),
            overwrite: false,
        })
    );
}

#[test]
fn extract_uses_default_pdfium_library_path() {
    let parsed = TermpdfCommand::parse_for_tests(
        ["termpdf", "extract", "sample.pdf"],
        Some(PathBuf::from("/env/pdfium")),
    )
    .unwrap();

    assert_eq!(
        parsed,
        TermpdfCommand::Extract(ExtractOptions {
            pdf_path: PathBuf::from("sample.pdf"),
            pdfium_lib_path: Some(PathBuf::from("/env/pdfium")),
            output_dir: PathBuf::from("sample.layout"),
            overwrite: false,
        })
    );
}

#[test]
fn parses_grep_command_defaults_to_regex_human_output() {
    let parsed =
        TermpdfCommand::parse_for_tests(["termpdf", "grep", "alpha beta", "sample.layout"], None)
            .unwrap();

    assert_eq!(
        parsed,
        TermpdfCommand::Grep(GrepOptions {
            layout_dir: PathBuf::from("sample.layout"),
            pattern: "alpha beta".to_string(),
            ignore_case: false,
            literal: false,
            json: false,
            refs_only: false,
        })
    );
}

#[test]
fn parses_grep_command_flags() {
    let parsed = TermpdfCommand::parse_for_tests(
        [
            "termpdf",
            "grep",
            "alpha.*beta",
            "sample.layout",
            "--ignore-case",
            "--json",
        ],
        None,
    )
    .unwrap();

    assert_eq!(
        parsed,
        TermpdfCommand::Grep(GrepOptions {
            layout_dir: PathBuf::from("sample.layout"),
            pattern: "alpha.*beta".to_string(),
            ignore_case: true,
            literal: false,
            json: true,
            refs_only: false,
        })
    );
}

#[test]
fn parses_grep_literal_flag() {
    let parsed = TermpdfCommand::parse_for_tests(
        [
            "termpdf",
            "grep",
            "alpha|beta",
            "sample.layout",
            "--literal",
        ],
        None,
    )
    .unwrap();

    assert_eq!(
        parsed,
        TermpdfCommand::Grep(GrepOptions {
            layout_dir: PathBuf::from("sample.layout"),
            pattern: "alpha|beta".to_string(),
            ignore_case: false,
            literal: true,
            json: false,
            refs_only: false,
        })
    );
}

#[test]
fn grep_rejects_incompatible_or_pdf_only_flags() {
    assert!(
        TermpdfCommand::parse_for_tests(
            [
                "termpdf",
                "grep",
                "alpha",
                "sample.layout",
                "--json",
                "--refs-only"
            ],
            None,
        )
        .is_err()
    );
    assert!(
        TermpdfCommand::parse_for_tests(
            [
                "termpdf",
                "--pdfium-lib",
                "/opt/pdfium",
                "grep",
                "alpha",
                "sample.layout"
            ],
            None,
        )
        .is_err()
    );
    assert!(
        TermpdfCommand::parse_for_tests(
            ["termpdf", "--watch", "grep", "alpha", "sample.layout"],
            None,
        )
        .is_err()
    );
}

#[test]
fn missing_view_pdf_path_is_an_error() {
    assert!(TermpdfCommand::parse_for_tests(["termpdf"], None).is_err());
}

#[test]
fn extract_rejects_viewer_only_flags() {
    assert!(
        TermpdfCommand::parse_for_tests(["termpdf", "--watch", "extract", "sample.pdf"], None,)
            .is_err()
    );
    assert!(
        TermpdfCommand::parse_for_tests(["termpdf", "--dark", "extract", "sample.pdf"], None,)
            .is_err()
    );
}
