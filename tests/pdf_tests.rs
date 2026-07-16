use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use termpdf::document::PdfRect;
use termpdf::pdf::PdfBackend;
use termpdf::pdf::{PdfBackendOptions, resolve_pdfium_lib_path_for_tests};
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

#[test]
fn extracts_top_level_and_nested_form_images_as_png() {
    let _pdfium_guard = pdfium_test_guard();
    let Ok(backend) = PdfBackend::new(None) else {
        eprintln!("skipping PDF image extraction test because PDFium is unavailable");
        return;
    };
    let temp = tempfile::tempdir().unwrap();
    let pdf_path = temp.path().join("nested-images.pdf");
    write_nested_image_pdf(&pdf_path);

    let session = backend.open_session(&pdf_path).unwrap();
    let images = &session.document().pages[0].images;

    assert_eq!(images.len(), 2);
    let top_level = images
        .iter()
        .find(|image| image.object_path.len() == 1)
        .unwrap();
    assert_eq!((top_level.pixel_width, top_level.pixel_height), (2, 1));
    assert_rect_close(top_level.bbox, PdfRect::new(30.0, 40.0, 20.0, 10.0));

    let nested = images
        .iter()
        .find(|image| image.object_path.len() == 3)
        .unwrap();
    assert_eq!((nested.pixel_width, nested.pixel_height), (1, 2));
    assert_rect_close(nested.bbox, PdfRect::new(181.0, 252.0, 24.0, 24.0));

    let assets = session.extract_image_assets().unwrap();
    assert_eq!(assets.len(), 2);
    for asset in assets {
        assert!(asset.png.starts_with(b"\x89PNG\r\n\x1a\n"));
        let decoded = image::load_from_memory(&asset.png).unwrap();
        assert!(decoded.width() > 0);
        assert!(decoded.height() > 0);
    }
}

#[test]
fn extracts_two_columns_in_column_reading_order() {
    let _pdfium_guard = pdfium_test_guard();
    let Ok(backend) = PdfBackend::new(None) else {
        eprintln!("skipping PDF text extraction test because PDFium is unavailable");
        return;
    };
    let temp = tempfile::tempdir().unwrap();
    let pdf_path = temp.path().join("two-columns.pdf");
    write_two_column_pdf(&pdf_path);

    let session = backend.open_session(&pdf_path).unwrap();
    let page = &session.document().pages[0];
    let lines = page
        .lines
        .iter()
        .map(|line| line.text())
        .collect::<Vec<_>>();

    assert_eq!(
        lines,
        [
            "Shared title",
            "Left heading",
            "Left first line remains in the left column.",
            "Left second line remains in the left column.",
            "Left third line remains in the left column.",
            "Right heading",
            "Right first line remains in the right column.",
            "Right second line remains in the right column.",
            "Right third line remains in the right column.",
        ]
    );
    assert!(page.lines.iter().all(line_contains_glyphs));
}

#[test]
fn extracts_short_spanning_heading_between_column_regions() {
    let _pdfium_guard = pdfium_test_guard();
    let Ok(backend) = PdfBackend::new(None) else {
        eprintln!("skipping PDF text extraction test because PDFium is unavailable");
        return;
    };
    let temp = tempfile::tempdir().unwrap();
    let pdf_path = temp.path().join("short-spanning-heading.pdf");
    write_short_spanning_heading_pdf(&pdf_path);

    let session = backend.open_session(&pdf_path).unwrap();
    let page = &session.document().pages[0];
    let lines = page
        .lines
        .iter()
        .map(|line| line.text().trim_end().to_owned())
        .collect::<Vec<_>>();

    assert_eq!(
        lines,
        [
            "Left body line one remains in the left column.",
            "Left body line two remains in the left column.",
            "Left body line three remains in the left column.",
            "Right body line one remains in the right column.",
            "Right body line two remains in the right column.",
            "Right body line three remains in the right column.",
            "Short section heading",
            "Left body line four remains in the left column.",
            "Left body line five remains in the left column.",
            "Left body line six remains in the left column.",
            "Right body line four remains in the right column.",
            "Right body line five remains in the right column.",
            "Right body line six remains in the right column.",
        ]
    );
}

fn pdfium_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static PDFIUM_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    PDFIUM_TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn write_two_column_pdf(path: &Path) {
    let content = b"BT /F1 18 Tf 230 750 Td (Shared title) Tj ET\n\
BT /F1 10 Tf 330 660 Td (Right second line remains in the right column.) Tj ET\n\
BT /F1 10 Tf 50 680 Td (Left first line remains in the left column.) Tj ET\n\
BT /F1 12 Tf 330 700 Td (Right heading) Tj ET\n\
BT /F1 10 Tf 330 680 Td (Right first line remains in the right column.) Tj ET\n\
BT /F1 10 Tf 330 640 Td (Right third line remains in the right column.) Tj ET\n\
BT /F1 10 Tf 50 660 Td (Left second line remains in the left column.) Tj ET\n\
BT /F1 10 Tf 50 640 Td (Left third line remains in the left column.) Tj ET\n\
BT /F1 12 Tf 50 700 Td (Left heading) Tj ET";
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        pdf_stream(b"<<", content),
    ];

    write_pdf_objects(path, &objects);
}

fn write_short_spanning_heading_pdf(path: &Path) {
    let content =
        b"BT /F1 10 Tf 330 660 Td (Right body line two remains in the right column.) Tj ET\n\
BT /F1 10 Tf 50 680 Td (Left body line one remains in the left column.) Tj ET\n\
BT /F1 10 Tf 330 640 Td (Right body line three remains in the right column.) Tj ET\n\
BT /F1 10 Tf 50 660 Td (Left body line two remains in the left column.) Tj ET\n\
BT /F1 14 Tf 265 620 Td (Short section heading) Tj ET\n\
BT /F1 10 Tf 50 640 Td (Left body line three remains in the left column.) Tj ET\n\
BT /F1 10 Tf 330 600 Td (Right body line four remains in the right column.) Tj ET\n\
BT /F1 10 Tf 50 600 Td (Left body line four remains in the left column.) Tj ET\n\
BT /F1 10 Tf 330 580 Td (Right body line five remains in the right column.) Tj ET\n\
BT /F1 10 Tf 50 580 Td (Left body line five remains in the left column.) Tj ET\n\
BT /F1 10 Tf 330 560 Td (Right body line six remains in the right column.) Tj ET\n\
BT /F1 10 Tf 50 560 Td (Left body line six remains in the left column.) Tj ET\n\
BT /F1 10 Tf 330 680 Td (Right body line one remains in the right column.) Tj ET";
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        pdf_stream(b"<<", content),
    ];

    write_pdf_objects(path, &objects);
}

fn write_nested_image_pdf(path: &Path) {
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /XObject << /Top 4 0 R /Outer 6 0 R >> >> /Contents 8 0 R >>".to_vec(),
        pdf_stream(
            b"<< /Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8",
            &[255, 0, 0, 0, 255, 0],
        ),
        pdf_stream(
            b"<< /Type /XObject /Subtype /Image /Width 1 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8",
            &[0, 0, 255, 255, 255, 0],
        ),
        pdf_stream(
            b"<< /Type /XObject /Subtype /Form /FormType 1 /BBox [0 0 1 1] /Resources << /XObject << /Inner 7 0 R >> >>",
            b"q 2 0 0 2 5 7 cm /Inner Do Q",
        ),
        pdf_stream(
            b"<< /Type /XObject /Subtype /Form /FormType 1 /BBox [0 0 1 1] /Resources << /XObject << /Img 5 0 R >> >>",
            b"q 4 0 0 3 11 13 cm /Img Do Q",
        ),
        pdf_stream(
            b"<<",
            b"q 20 0 0 10 30 40 cm /Top Do Q q 3 0 0 4 100 120 cm /Outer Do Q",
        ),
    ];

    write_pdf_objects(path, &objects);
}

fn write_pdf_objects(path: &Path, objects: &[Vec<u8>]) {
    let mut pdf = b"%PDF-1.7\n%\x80\x81\x82\x83\n".to_vec();
    let mut offsets = vec![0];
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        pdf.extend_from_slice(object);
        pdf.extend_from_slice(b"\nendobj\n");
    }

    let xref_offset = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    pdf.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets.into_iter().skip(1) {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    std::fs::write(path, pdf).unwrap();
}

fn pdf_stream(dictionary: &[u8], data: &[u8]) -> Vec<u8> {
    let mut object = dictionary.to_vec();
    object.extend_from_slice(format!(" /Length {} >>\nstream\n", data.len()).as_bytes());
    object.extend_from_slice(data);
    object.extend_from_slice(b"\nendstream");
    object
}

fn assert_rect_close(actual: PdfRect, expected: PdfRect) {
    assert!((actual.x - expected.x).abs() < 0.01, "x: {actual:?}");
    assert!((actual.y - expected.y).abs() < 0.01, "y: {actual:?}");
    assert!(
        (actual.width - expected.width).abs() < 0.01,
        "width: {actual:?}"
    );
    assert!(
        (actual.height - expected.height).abs() < 0.01,
        "height: {actual:?}"
    );
}

fn line_contains_glyphs(line: &termpdf::document::PdfLine) -> bool {
    line.glyphs.iter().all(|glyph| {
        glyph.bbox.x >= line.bbox.x
            && glyph.bbox.y >= line.bbox.y
            && glyph.bbox.x + glyph.bbox.width <= line.bbox.x + line.bbox.width
            && glyph.bbox.y + glyph.bbox.height <= line.bbox.y + line.bbox.height
    })
}
