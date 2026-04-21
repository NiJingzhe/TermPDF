use std::path::PathBuf;

pub const DEV_CONFIG_FILE_NAME: &str = "termpdf.dev.toml";
pub const PDFIUM_RELEASE_TAG: &str = "chromium/7789";
pub const PDFIUM_VERSION: &str = "149.0.7789.0";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BundledPdfiumVariant {
    pub feature_name: &'static str,
    pub config_name: &'static str,
    pub platform_archive_stem: &'static str,
    pub library_name: &'static str,
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

pub fn bundled_pdfium_variant(os: &str, arch: &str) -> Option<BundledPdfiumVariant> {
    let config_name = match (os, arch) {
        ("macos", "aarch64") => "macos-arm64",
        ("linux", "x86_64") => "linux-x64-glibc",
        ("linux", "arm") | ("linux", "armv7") | ("linux", "armv7l") => {
            "linux-arm-glibc"
        }
        ("linux", "aarch64") => "linux-arm64-glibc",
        _ => return None,
    };

    bundled_pdfium_variant_by_config(config_name).ok()
}

pub fn packaged_pdfium_library_name(os: &str) -> Option<&'static str> {
    match os {
        "linux" => Some("libpdfium.so"),
        "macos" => Some("libpdfium.dylib"),
        _ => None,
    }
}

pub fn select_bundled_pdfium_variant<'a, I>(
    enabled_features: I,
) -> Result<Option<BundledPdfiumVariant>, String>
where
    I: IntoIterator<Item = &'a str>,
{
    let enabled = enabled_features.into_iter().collect::<Vec<_>>();
    let selected = BUNDLED_PDFIUM_VARIANTS
        .iter()
        .copied()
        .filter(|variant| {
            enabled
                .iter()
                .any(|feature| *feature == variant.feature_name)
        })
        .collect::<Vec<_>>();

    match selected.as_slice() {
        [] => Ok(None),
        [variant] => Ok(Some(*variant)),
        _ => Err(
            "multiple PDFium bundle features enabled; select exactly one bundle variant"
                .to_string(),
        ),
    }
}

pub fn bundled_pdfium_variant_by_config(
    config_name: &str,
) -> Result<BundledPdfiumVariant, String> {
    BUNDLED_PDFIUM_VARIANTS
        .iter()
        .copied()
        .find(|variant| variant.config_name == config_name)
        .ok_or_else(|| format!("unsupported PDFium variant '{config_name}'"))
}

pub fn dev_config_variant_from_contents(
    contents: &str,
) -> Result<Option<BundledPdfiumVariant>, String> {
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

        return bundled_pdfium_variant_by_config(value).map(Some);
    }

    Ok(None)
}

pub fn pdfium_cache_root(project_root: PathBuf) -> PathBuf {
    project_root.join(".cache").join("pdfium")
}

pub fn pdfium_extracted_dir(project_root: PathBuf, variant: BundledPdfiumVariant) -> PathBuf {
    pdfium_cache_root(project_root)
        .join(PDFIUM_RELEASE_TAG.replace('/', "-"))
        .join(variant.config_name)
}

pub fn pdfium_archive_name(variant: BundledPdfiumVariant) -> String {
    format!("pdfium-{}.tgz", variant.platform_archive_stem)
}

pub fn supported_config_names() -> Vec<&'static str> {
    BUNDLED_PDFIUM_VARIANTS
        .iter()
        .map(|variant| variant.config_name)
        .collect()
}
