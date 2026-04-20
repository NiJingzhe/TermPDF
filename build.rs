use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const DEV_CONFIG_FILE_NAME: &str = "termpdf.dev.toml";

#[derive(Clone, Copy)]
struct BundledPdfiumVariant {
    feature_name: &'static str,
    config_name: &'static str,
    vendor_dir: &'static str,
    library_name: &'static str,
}

const BUNDLED_PDFIUM_VARIANTS: &[BundledPdfiumVariant] = &[
    BundledPdfiumVariant {
        feature_name: "bundle-pdfium-linux-x64-glibc",
        config_name: "linux-x64-glibc",
        vendor_dir: "linux-x64-glibc",
        library_name: "libpdfium.so",
    },
    BundledPdfiumVariant {
        feature_name: "bundle-pdfium-linux-x86-glibc",
        config_name: "linux-x86-glibc",
        vendor_dir: "linux-x86-glibc",
        library_name: "libpdfium.so",
    },
    BundledPdfiumVariant {
        feature_name: "bundle-pdfium-linux-arm-glibc",
        config_name: "linux-arm-glibc",
        vendor_dir: "linux-arm-glibc",
        library_name: "libpdfium.so",
    },
    BundledPdfiumVariant {
        feature_name: "bundle-pdfium-linux-arm64-glibc",
        config_name: "linux-arm64-glibc",
        vendor_dir: "linux-arm64-glibc",
        library_name: "libpdfium.so",
    },
    BundledPdfiumVariant {
        feature_name: "bundle-pdfium-linux-ppc64-glibc",
        config_name: "linux-ppc64-glibc",
        vendor_dir: "linux-ppc64-glibc",
        library_name: "libpdfium.so",
    },
    BundledPdfiumVariant {
        feature_name: "bundle-pdfium-macos-arm64",
        config_name: "macos-arm64",
        vendor_dir: "macos-arm64",
        library_name: "libpdfium.dylib",
    },
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=vendor/pdfium");
    println!("cargo:rerun-if-changed={DEV_CONFIG_FILE_NAME}");
    println!("cargo:rerun-if-env-changed=TERMPDF_PDFIUM_VARIANT");

    let Some(variant) = selected_variant() else {
        return;
    };

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let target_dir = target_dir_from_out_dir(&out_dir);
    let profile = env::var("PROFILE").expect("PROFILE");
    let source = manifest_dir
        .join("vendor/pdfium")
        .join(variant.vendor_dir)
        .join("lib")
        .join(variant.library_name);
    let destination = target_dir.join(&profile).join(variant.library_name);

    fs::create_dir_all(destination.parent().expect("library destination parent"))
        .expect("create output directory for bundled PDFium");
    fs::copy(&source, &destination).unwrap_or_else(|error| {
        panic!(
            "failed to copy bundled PDFium from {} to {}: {error}",
            source.display(),
            destination.display(),
        )
    });
}

fn selected_variant() -> Option<BundledPdfiumVariant> {
    if let Some(config_name) = dev_config_pdfium_variant() {
        return Some(variant_by_config_name(&config_name).unwrap_or_else(|| {
            panic!(
                "unsupported pdfium_variant='{config_name}' in {DEV_CONFIG_FILE_NAME}; expected one of: {}",
                supported_config_names().join(", ")
            )
        }));
    }

    if let Some(config_name) = env::var_os("TERMPDF_PDFIUM_VARIANT") {
        let config_name = config_name.to_string_lossy();
        return Some(variant_by_config_name(&config_name).unwrap_or_else(|| {
            panic!(
                "unsupported TERMPDF_PDFIUM_VARIANT='{config_name}'; expected one of: {}",
                supported_config_names().join(", ")
            )
        }));
    }

    let enabled = BUNDLED_PDFIUM_VARIANTS
        .iter()
        .copied()
        .filter(|variant| feature_enabled(variant.feature_name))
        .collect::<Vec<_>>();

    if enabled.len() > 1 {
        panic!("multiple PDFium bundle features enabled; select exactly one bundle variant");
    }

    enabled.first().copied()
}

fn dev_config_pdfium_variant() -> Option<String> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let config_path = manifest_dir.join(DEV_CONFIG_FILE_NAME);
    let contents = fs::read_to_string(config_path).ok()?;

    parse_dev_config_pdfium_variant(&contents).unwrap_or_else(|error| {
        panic!("invalid {DEV_CONFIG_FILE_NAME}: {error}");
    })
}

fn feature_enabled(feature_name: &str) -> bool {
    let env_name = format!(
        "CARGO_FEATURE_{}",
        feature_name.replace('-', "_").to_ascii_uppercase()
    );
    env::var_os(env_name).is_some()
}

fn variant_by_config_name(config_name: &str) -> Option<BundledPdfiumVariant> {
    BUNDLED_PDFIUM_VARIANTS
        .iter()
        .copied()
        .find(|variant| variant.config_name == config_name)
}

fn supported_config_names() -> Vec<&'static str> {
    BUNDLED_PDFIUM_VARIANTS
        .iter()
        .map(|variant| variant.config_name)
        .collect()
}

fn parse_dev_config_pdfium_variant(contents: &str) -> Result<Option<String>, String> {
    let mut in_root = true;

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if trimmed.starts_with('[') {
            in_root = false;
            continue;
        }

        if !in_root {
            continue;
        }

        let Some((key, raw_value)) = trimmed.split_once('=') else {
            continue;
        };
        if key.trim() != "pdfium_variant" {
            continue;
        }

        let raw_value = raw_value.split('#').next().unwrap_or("").trim();
        let Some(value) = raw_value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
        else {
            return Err("pdfium_variant must be a double-quoted string".to_string());
        };

        return Ok(Some(value.to_string()));
    }

    Ok(None)
}

fn target_dir_from_out_dir(out_dir: &Path) -> PathBuf {
    let mut cursor = out_dir.to_path_buf();
    for _ in 0..4 {
        cursor = cursor
            .parent()
            .expect("OUT_DIR should be nested under target/<profile>/build")
            .to_path_buf();
    }
    cursor
}
