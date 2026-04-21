use std::path::PathBuf;

use termpdf::pdfium_bundle::{
    DEV_CONFIG_FILE_NAME, bundled_pdfium_variant, bundled_pdfium_variant_by_config,
    dev_config_variant_from_contents, packaged_pdfium_library_name, pdfium_archive_name,
    pdfium_extracted_dir, select_bundled_pdfium_variant,
};

#[test]
fn resolves_supported_architectures_to_pdfium_variants() {
    assert_eq!(
        bundled_pdfium_variant("linux", "x86_64").unwrap().config_name,
        "linux-x64-glibc"
    );
    assert_eq!(
        bundled_pdfium_variant("linux", "arm").unwrap().config_name,
        "linux-arm-glibc"
    );
    assert_eq!(
        bundled_pdfium_variant("linux", "aarch64").unwrap().config_name,
        "linux-arm64-glibc"
    );
}

#[test]
fn selects_requested_linux_bundle_variant() {
    let variant = select_bundled_pdfium_variant(["bundle-pdfium-linux-arm64-glibc"])
        .unwrap()
        .unwrap();

    assert_eq!(variant.feature_name, "bundle-pdfium-linux-arm64-glibc");
    assert_eq!(variant.config_name, "linux-arm64-glibc");
    assert_eq!(variant.platform_archive_stem, "linux-arm64");
    assert_eq!(variant.library_name, "libpdfium.so");
}

#[test]
fn rejects_multiple_bundle_variants() {
    let error = select_bundled_pdfium_variant([
        "bundle-pdfium-linux-x64-glibc",
        "bundle-pdfium-linux-arm64-glibc",
    ])
    .unwrap_err();

    assert!(error.contains("multiple PDFium bundle features"));
}

#[test]
fn resolves_packaged_library_names_for_supported_platforms() {
    assert_eq!(packaged_pdfium_library_name("linux"), Some("libpdfium.so"));
    assert_eq!(
        packaged_pdfium_library_name("macos"),
        Some("libpdfium.dylib")
    );
}

#[test]
fn resolves_bundle_variant_from_config_name() {
    let variant = bundled_pdfium_variant_by_config("linux-x64-glibc").unwrap();

    assert_eq!(variant.feature_name, "bundle-pdfium-linux-x64-glibc");
    assert_eq!(variant.platform_archive_stem, "linux-x64");
}

#[test]
fn parses_pdfium_variant_from_dev_config() {
    let variant = dev_config_variant_from_contents(
        "# local development config\npdfium_variant = \"linux-arm64-glibc\"\n",
    )
    .unwrap()
    .unwrap();

    assert_eq!(variant.config_name, "linux-arm64-glibc");
}

#[test]
fn derives_archive_name_from_variant() {
    let variant = bundled_pdfium_variant_by_config("macos-arm64").unwrap();

    assert_eq!(pdfium_archive_name(variant), "pdfium-mac-arm64.tgz");
}

#[test]
fn derives_extracted_cache_path_from_variant() {
    let variant = bundled_pdfium_variant_by_config("linux-x64-glibc").unwrap();

    assert_eq!(
        pdfium_extracted_dir(PathBuf::from("/workspace/project"), variant),
        PathBuf::from("/workspace/project/.cache/pdfium/chromium-7789/linux-x64-glibc")
    );
}

#[test]
fn config_file_name_is_stable() {
    assert_eq!(DEV_CONFIG_FILE_NAME, "termpdf.dev.toml");
}
