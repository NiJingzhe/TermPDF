use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const DEV_CONFIG_FILE_NAME: &str = "termpdf.dev.toml";
const PDFIUM_RELEASE_TAG: &str = "chromium/7789";
const PDFIUM_RELEASE_REPO: &str = "bblanchon/pdfium-binaries";

#[derive(Clone, Copy)]
struct BundledPdfiumVariant {
    feature_name: &'static str,
    config_name: &'static str,
    platform_archive_stem: &'static str,
    library_name: &'static str,
}

const BUNDLED_PDFIUM_VARIANTS: &[BundledPdfiumVariant] = &[
    BundledPdfiumVariant {
        feature_name: "bundle-pdfium-linux-x64-glibc",
        config_name: "linux-x64-glibc",
        platform_archive_stem: "linux-x64",
        library_name: "libpdfium.so",
    },
    BundledPdfiumVariant {
        feature_name: "bundle-pdfium-linux-arm-glibc",
        config_name: "linux-arm-glibc",
        platform_archive_stem: "linux-arm",
        library_name: "libpdfium.so",
    },
    BundledPdfiumVariant {
        feature_name: "bundle-pdfium-linux-arm64-glibc",
        config_name: "linux-arm64-glibc",
        platform_archive_stem: "linux-arm64",
        library_name: "libpdfium.so",
    },
    BundledPdfiumVariant {
        feature_name: "bundle-pdfium-macos-arm64",
        config_name: "macos-arm64",
        platform_archive_stem: "mac-arm64",
        library_name: "libpdfium.dylib",
    },
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={DEV_CONFIG_FILE_NAME}");
    println!("cargo:rerun-if-env-changed=TERMPDF_PDFIUM_VARIANT");

    let Some(variant) = selected_variant() else {
        return;
    };

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let target_dir = target_dir_from_out_dir(&out_dir);
    let profile = env::var("PROFILE").expect("PROFILE");
    let source = ensure_pdfium_library(&manifest_dir, variant);
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

fn ensure_pdfium_library(project_root: &Path, variant: BundledPdfiumVariant) -> PathBuf {
    let extracted_dir = pdfium_extracted_dir(project_root, variant);
    let library_path = extracted_dir.join("lib").join(variant.library_name);
    if library_path.exists() {
        return library_path;
    }

    let cache_root = extracted_dir.parent().expect("pdfium cache tag dir");
    fs::create_dir_all(cache_root).expect("create pdfium cache dir");
    let archive_name = format!("pdfium-{}.tgz", variant.platform_archive_stem);
    let archive_path = cache_root.join(&archive_name);

    download_pdfium_archive(&archive_name, &archive_path);
    extract_pdfium_archive(&archive_path, &extracted_dir);

    if !library_path.exists() {
        panic!(
            "downloaded PDFium archive did not contain expected library at {}",
            library_path.display()
        );
    }

    library_path
}

fn download_pdfium_archive(archive_name: &str, archive_path: &Path) {
    if archive_path.exists() {
        return;
    }

    let status = Command::new("gh")
        .args([
            "release",
            "download",
            PDFIUM_RELEASE_TAG,
            "-R",
            PDFIUM_RELEASE_REPO,
            "-p",
            archive_name,
            "-O",
        ])
        .arg(archive_path)
        .status()
        .unwrap_or_else(|error| panic!("failed to invoke gh for PDFium download: {error}"));
    if status.success() {
        return;
    }

    let url = format!(
        "https://github.com/{PDFIUM_RELEASE_REPO}/releases/download/{PDFIUM_RELEASE_TAG}/{archive_name}"
    );
    let status = Command::new("curl")
        .args(["-L", "--fail", "--output"])
        .arg(archive_path)
        .arg(url)
        .status()
        .unwrap_or_else(|error| panic!("failed to invoke curl for PDFium download: {error}"));
    if !status.success() {
        panic!("failed to download PDFium archive {archive_name}");
    }
}

fn extract_pdfium_archive(archive_path: &Path, extracted_dir: &Path) {
    if extracted_dir.exists() {
        fs::remove_dir_all(extracted_dir).expect("remove incomplete extracted PDFium dir");
    }
    fs::create_dir_all(extracted_dir).expect("create extracted PDFium dir");

    let status = Command::new("tar")
        .args(["-xzf"])
        .arg(archive_path)
        .args(["-C"])
        .arg(extracted_dir)
        .status()
        .unwrap_or_else(|error| panic!("failed to invoke tar for PDFium extraction: {error}"));
    if !status.success() {
        panic!("failed to extract PDFium archive {}", archive_path.display());
    }
}

fn selected_variant() -> Option<BundledPdfiumVariant> {
    if let Some(variant) = dev_config_pdfium_variant() {
        return Some(variant);
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

fn dev_config_pdfium_variant() -> Option<BundledPdfiumVariant> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let config_path = manifest_dir.join(DEV_CONFIG_FILE_NAME);
    let contents = fs::read_to_string(config_path).ok()?;

    parse_dev_config(&contents).unwrap_or_else(|error| {
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

fn parse_dev_config(contents: &str) -> Result<Option<BundledPdfiumVariant>, String> {
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

        return variant_by_config_name(value)
            .ok_or_else(|| {
                format!(
                    "unsupported pdfium_variant='{value}' in {DEV_CONFIG_FILE_NAME}; expected one of: {}",
                    supported_config_names().join(", ")
                )
            })
            .map(Some);
    }

    Ok(None)
}

fn pdfium_extracted_dir(project_root: &Path, variant: BundledPdfiumVariant) -> PathBuf {
    project_root
        .join(".cache")
        .join("pdfium")
        .join(PDFIUM_RELEASE_TAG.replace('/', "-"))
        .join(variant.config_name)
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
