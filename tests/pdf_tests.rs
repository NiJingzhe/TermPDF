use std::path::PathBuf;

use termpdf::document::PdfRect;
use termpdf::pdf::{resolve_pdfium_lib_path_for_tests, PdfBackendOptions};
use termpdf::render::{PageRenderCache, PageRenderInfo, RenderedPage};

#[test]
fn groups_glyphs_into_visual_lines_smoke() {
    let page = termpdf::document::Page::from_text(0, &["ab", "cd"]);

    assert_eq!(page.lines.len(), 2);
    assert_eq!(page.lines[0].text(), "ab");
    assert_eq!(page.lines[1].text(), "cd");
}

#[test]
fn pdf_backend_options_parse_pdf_and_library_path() {
    let parsed = PdfBackendOptions::from_args_fallback_for_tests(
        ["sample.pdf", "--pdfium-lib", "/opt/pdfium"],
        None,
    )
    .unwrap();

    assert_eq!(parsed.pdf_path, PathBuf::from("sample.pdf"));
    assert_eq!(parsed.pdfium_lib_path, Some(PathBuf::from("/opt/pdfium")));
    assert!(!parsed.dark_mode);
}

#[test]
fn pdf_backend_options_fall_back_to_env_library_path() {
    let parsed = PdfBackendOptions::from_args_fallback_for_tests(
        ["sample.pdf"],
        Some(PathBuf::from("/env/pdfium")),
    )
    .unwrap();

    assert_eq!(parsed.pdf_path, PathBuf::from("sample.pdf"));
    assert_eq!(parsed.pdfium_lib_path, Some(PathBuf::from("/env/pdfium")));
    assert!(!parsed.dark_mode);
}

#[test]
fn pdf_backend_options_override_env_with_cli_library_path() {
    let parsed = PdfBackendOptions::from_args_fallback_for_tests(
        ["sample.pdf", "--pdfium-lib", "/tmp/libpdfium.dylib"],
        Some(PathBuf::from("/env/pdfium")),
    )
    .unwrap();

    assert_eq!(parsed.pdf_path, PathBuf::from("sample.pdf"));
    assert_eq!(
        parsed.pdfium_lib_path,
        Some(PathBuf::from("/tmp/libpdfium.dylib"))
    );
    assert!(!parsed.dark_mode);
}

#[test]
fn pdf_backend_options_parse_dark_flag() {
    let parsed =
        PdfBackendOptions::from_args_fallback_for_tests(["sample.pdf", "--dark"], None).unwrap();

    assert_eq!(parsed.pdf_path, PathBuf::from("sample.pdf"));
    assert!(parsed.dark_mode);
    assert!(!parsed.watch_mode);
}

#[test]
fn pdf_backend_options_parse_watch_flag_after_file_path() {
    let parsed =
        PdfBackendOptions::from_args_fallback_for_tests(["sample.pdf", "-w"], None).unwrap();

    assert_eq!(parsed.pdf_path, PathBuf::from("sample.pdf"));
    assert!(parsed.watch_mode);
}

#[test]
fn pdf_backend_options_parse_long_watch_flag() {
    let parsed =
        PdfBackendOptions::from_args_fallback_for_tests(["sample.pdf", "--watch"], None).unwrap();

    assert_eq!(parsed.pdf_path, PathBuf::from("sample.pdf"));
    assert!(parsed.watch_mode);
}

#[test]
fn pdf_session_cache_contains_rendered_key() {
    let mut cache = PageRenderCache::default();
    let key = PageRenderInfo {
        page_index: 1,
        placement_col: 0,
        placement_row: 0,
        bitmap_width: 200,
        bitmap_height: 300,
        crop_x: 0,
        crop_y: 0,
        crop_width: 200,
        crop_height: 300,
        placement_columns: 20,
        placement_rows: 15,
    };

    cache
        .get_or_insert_with(key, || {
            Ok::<_, ()>(RenderedPage {
                page_index: 1,
                placement_col: 0,
                placement_row: 0,
                bitmap_width: 200,
                bitmap_height: 300,
                crop_x: 0,
                crop_y: 0,
                crop_width: 200,
                crop_height: 300,
                placement_columns: 20,
                placement_rows: 15,
                rgba: vec![0, 0, 0, 0],
            })
        })
        .unwrap();

    assert!(cache.contains(key));
    assert_eq!(
        PdfRect::new(0.0, 0.0, 1.0, 1.0),
        PdfRect::new(0.0, 0.0, 1.0, 1.0)
    );
}

#[test]
fn bundled_pdfium_lookup_prefers_explicit_path() {
    let resolved = resolve_pdfium_lib_path_for_tests(
        Some(PathBuf::from("/tmp/custom/libpdfium.dylib")),
        None,
        None,
        PathBuf::from("/workspace/project"),
        "macos",
        "aarch64",
    );

    assert_eq!(resolved, Some(PathBuf::from("/tmp/custom/libpdfium.dylib")));
}

#[test]
fn bundled_pdfium_lookup_uses_project_cache_directory_for_macos_arm64() {
    let resolved = resolve_pdfium_lib_path_for_tests(
        None,
        None,
        None,
        PathBuf::from("/workspace/project"),
        "macos",
        "aarch64",
    );

    assert_eq!(
        resolved,
        Some(PathBuf::from(
            "/workspace/project/.cache/pdfium/chromium-7789/macos-arm64/lib/libpdfium.dylib"
        ))
    );
}

#[test]
fn bundled_pdfium_lookup_uses_project_cache_directory_for_linux_glibc_x64() {
    let resolved = resolve_pdfium_lib_path_for_tests(
        None,
        None,
        None,
        PathBuf::from("/workspace/project"),
        "linux",
        "x86_64",
    );

    assert_eq!(
        resolved,
        Some(PathBuf::from(
            "/workspace/project/.cache/pdfium/chromium-7789/linux-x64-glibc/lib/libpdfium.so"
        ))
    );
}

#[test]
fn bundled_pdfium_lookup_prefers_packaged_library_next_to_binary() {
    let resolved = resolve_pdfium_lib_path_for_tests(
        None,
        None,
        Some(PathBuf::from("/dist/libpdfium.so")),
        PathBuf::from("/workspace/project"),
        "linux",
        "x86_64",
    );

    assert_eq!(resolved, Some(PathBuf::from("/dist/libpdfium.so")));
}
