pub const DEV_CONFIG_FILE_NAME: &str = "termpdf.dev.toml";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BundledPdfiumVariant {
    pub feature_name: &'static str,
    pub config_name: &'static str,
    pub vendor_dir: &'static str,
    pub library_name: &'static str,
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

pub fn bundled_pdfium_vendor_dir(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("macos", "aarch64") => Some("macos-arm64"),
        ("linux", "x86_64") => Some("linux-x64-glibc"),
        ("linux", "x86") | ("linux", "i686") => Some("linux-x86-glibc"),
        ("linux", "arm") | ("linux", "armv7") | ("linux", "armv7l") => Some("linux-arm-glibc"),
        ("linux", "aarch64") => Some("linux-arm64-glibc"),
        ("linux", "powerpc64") | ("linux", "powerpc64le") => Some("linux-ppc64-glibc"),
        _ => None,
    }
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

pub fn bundled_pdfium_variant_by_config(config_name: &str) -> Result<BundledPdfiumVariant, String> {
    BUNDLED_PDFIUM_VARIANTS
        .iter()
        .copied()
        .find(|variant| variant.config_name == config_name)
        .ok_or_else(|| format!("unsupported PDFium variant '{config_name}'"))
}

pub fn dev_config_pdfium_variant_from_contents(contents: &str) -> Result<Option<String>, String> {
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
