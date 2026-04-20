use termpdf::pdfium_bundle::{
    bundled_pdfium_variant_by_config, bundled_pdfium_vendor_dir,
    dev_config_pdfium_variant_from_contents, packaged_pdfium_library_name,
    select_bundled_pdfium_variant, DEV_CONFIG_FILE_NAME,
};

#[test]
fn resolves_linux_glibc_vendor_dirs_for_supported_architectures() {
    assert_eq!(
        bundled_pdfium_vendor_dir("linux", "x86_64"),
        Some("linux-x64-glibc")
    );
    assert_eq!(
        bundled_pdfium_vendor_dir("linux", "x86"),
        Some("linux-x86-glibc")
    );
    assert_eq!(
        bundled_pdfium_vendor_dir("linux", "arm"),
        Some("linux-arm-glibc")
    );
    assert_eq!(
        bundled_pdfium_vendor_dir("linux", "aarch64"),
        Some("linux-arm64-glibc")
    );
    assert_eq!(
        bundled_pdfium_vendor_dir("linux", "powerpc64"),
        Some("linux-ppc64-glibc")
    );
}

#[test]
fn selects_requested_linux_bundle_variant() {
    let variant = select_bundled_pdfium_variant(["bundle-pdfium-linux-arm64-glibc"])
        .unwrap()
        .unwrap();

    assert_eq!(variant.feature_name, "bundle-pdfium-linux-arm64-glibc");
    assert_eq!(variant.config_name, "linux-arm64-glibc");
    assert_eq!(variant.vendor_dir, "linux-arm64-glibc");
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
    assert_eq!(variant.vendor_dir, "linux-x64-glibc");
}

#[test]
fn parses_pdfium_variant_from_dev_config() {
    let variant = dev_config_pdfium_variant_from_contents(
        "# local development config\npdfium_variant = \"linux-arm64-glibc\"\n",
    )
    .unwrap();

    assert_eq!(variant.as_deref(), Some("linux-arm64-glibc"));
}

#[test]
fn config_file_name_is_stable() {
    assert_eq!(DEV_CONFIG_FILE_NAME, "termpdf.dev.toml");
}
