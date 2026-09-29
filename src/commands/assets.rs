use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MANIFEST_NAME: &str = "assets.manifest.json";
const AUDIT_MANIFEST_NAME: &str = "bornengine.assets.json";
const AUDIT_REPORT_FORMAT: &str = "bornengine.asset_validation";
const MANIFEST_FORMAT: &str = "bornengine-assets-v1";
const WORLD_FORMAT: &str = "bornengine.world2d";
const WORLD_VERSION: u64 = 1;
const ASSET_ROOTS: [&str; 3] = ["assets", "public", "static"];
const SKIPPED_DIRECTORIES: [&str; 7] = [
    ".git",
    ".bornengine",
    ".perry-dev",
    "node_modules",
    "target",
    "dist",
    ".cache",
];

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct AssetSummary {
    pub files: usize,
    pub bytes: u64,
    #[serde(skip)]
    pub watch_directories: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
#[clap(rename_all = "kebab-case")]
pub enum OrphanPolicy {
    Ignore,
    #[default]
    Warn,
    Error,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetSeverity {
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetDiagnosticCode {
    OrphanAsset,
    InvalidMediaHeader,
    ExtensionMismatch,
    FileSizeLimit,
    TotalSizeLimit,
    ImageDimensionLimit,
    TotalImagePixelsLimit,
    DeclaredDynamicPathMissing,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AssetValidationOptions {
    pub orphan_policy: Option<OrphanPolicy>,
    pub max_file_bytes: Option<u64>,
    pub max_total_bytes: Option<u64>,
    pub max_image_dimension: Option<u32>,
    pub max_total_image_pixels: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetValidationSettings {
    pub dynamic_paths: Vec<String>,
    pub ignored_paths: Vec<String>,
    pub orphan_policy: OrphanPolicy,
    pub max_file_bytes: Option<u64>,
    pub max_total_bytes: Option<u64>,
    pub max_image_dimension: Option<u32>,
    pub max_total_image_pixels: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AssetDiagnostic {
    pub code: AssetDiagnosticCode,
    pub severity: AssetSeverity,
    pub path: String,
    pub message: String,
    pub measured: Option<u64>,
    pub limit: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AssetValidationReport {
    pub format: &'static str,
    pub version: u32,
    pub summary: AssetSummary,
    pub diagnostics: Vec<AssetDiagnostic>,
}

impl AssetValidationReport {
    pub fn new(summary: AssetSummary, mut diagnostics: Vec<AssetDiagnostic>) -> Result<Self> {
        for diagnostic in &diagnostics {
            if !diagnostic.path.is_empty() {
                validate_project_relative_path(&diagnostic.path)
                    .with_context(|| format!("unsafe diagnostic path `{}`", diagnostic.path))?;
            }
        }
        diagnostics.sort_by(|a, b| {
            (
                &a.path, &a.code, a.severity, &a.message, a.measured, a.limit,
            )
                .cmp(&(
                    &b.path, &b.code, b.severity, &b.message, b.measured, b.limit,
                ))
        });
        Ok(Self {
            format: AUDIT_REPORT_FORMAT,
            version: 1,
            summary,
            diagnostics,
        })
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == AssetSeverity::Error)
    }
}

pub fn load_asset_validation_settings(
    project_root: &Path,
    options: &AssetValidationOptions,
) -> Result<AssetValidationSettings> {
    let root = canonical_root(project_root)?;
    let manifest_path = root.join(AUDIT_MANIFEST_NAME);
    let document = match fs::symlink_metadata(&manifest_path) {
        Ok(_) => {
            let canonical = manifest_path
                .canonicalize()
                .with_context(|| format!("could not resolve {AUDIT_MANIFEST_NAME}"))?;
            ensure_inside_root(&root, &canonical, AUDIT_MANIFEST_NAME)?;
            let bytes = fs::read(&manifest_path)
                .with_context(|| format!("could not read {AUDIT_MANIFEST_NAME}"))?;
            Some(
                serde_json::from_slice::<Value>(&bytes)
                    .with_context(|| format!("invalid {AUDIT_MANIFEST_NAME}"))?,
            )
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error).with_context(|| format!("could not inspect {AUDIT_MANIFEST_NAME}"));
        }
    };
    let mut settings = AssetValidationSettings {
        dynamic_paths: Vec::new(),
        ignored_paths: Vec::new(),
        orphan_policy: OrphanPolicy::Warn,
        max_file_bytes: None,
        max_total_bytes: None,
        max_image_dimension: None,
        max_total_image_pixels: None,
    };
    if let Some(document) = document {
        let object = document
            .as_object()
            .ok_or_else(|| anyhow!("{AUDIT_MANIFEST_NAME}: expected a JSON object"))?;
        if object.get("version").and_then(Value::as_u64) != Some(1) {
            bail!("{AUDIT_MANIFEST_NAME}: unsupported version; expected version 1");
        }
        for key in object.keys() {
            if ![
                "version",
                "dynamic_paths",
                "ignored_paths",
                "orphan_severity",
                "max_file_bytes",
                "max_total_bytes",
                "max_image_dimension",
                "max_total_image_pixels",
            ]
            .contains(&key.as_str())
            {
                bail!("{AUDIT_MANIFEST_NAME}: unsupported field `{key}`");
            }
        }
        settings.dynamic_paths = manifest_paths_field(object, "dynamic_paths", false)?;
        settings.ignored_paths = manifest_paths_field(object, "ignored_paths", true)?;
        if let Some(value) = object.get("orphan_severity") {
            settings.orphan_policy = match value.as_str() {
                Some("ignore") => OrphanPolicy::Ignore,
                Some("warning") | Some("warn") => OrphanPolicy::Warn,
                Some("error") => OrphanPolicy::Error,
                _ => bail!("{AUDIT_MANIFEST_NAME}: invalid orphan_severity"),
            };
        }
        settings.max_file_bytes = positive_limit(object, "max_file_bytes")?;
        settings.max_total_bytes = positive_limit(object, "max_total_bytes")?;
        settings.max_total_image_pixels = positive_limit(object, "max_total_image_pixels")?;
        settings.max_image_dimension = positive_limit(object, "max_image_dimension")?
            .map(|value| {
                u32::try_from(value).map_err(|_| {
                    anyhow!("{AUDIT_MANIFEST_NAME}: max_image_dimension exceeds u32 range")
                })
            })
            .transpose()?;
    }
    if let Some(value) = options.orphan_policy {
        settings.orphan_policy = value;
    }
    if let Some(value) = options.max_file_bytes {
        settings.max_file_bytes = Some(checked_option_limit(value, "max_file_bytes")?);
    }
    if let Some(value) = options.max_total_bytes {
        settings.max_total_bytes = Some(checked_option_limit(value, "max_total_bytes")?);
    }
    if let Some(value) = options.max_image_dimension {
        settings.max_image_dimension = Some(u32::try_from(checked_option_limit(
            u64::from(value),
            "max_image_dimension",
        )?)?);
    }
    if let Some(value) = options.max_total_image_pixels {
        settings.max_total_image_pixels =
            Some(checked_option_limit(value, "max_total_image_pixels")?);
    }
    Ok(settings)
}

fn manifest_paths_field(
    object: &serde_json::Map<String, Value>,
    key: &str,
    globs: bool,
) -> Result<Vec<String>> {
    let Some(value) = object.get(key) else {
        return Ok(Vec::new());
    };
    let values = value
        .as_array()
        .ok_or_else(|| anyhow!("{AUDIT_MANIFEST_NAME}: `{key}` must be an array"))?;
    let mut paths = BTreeSet::new();
    for value in values {
        let path = value
            .as_str()
            .ok_or_else(|| anyhow!("{AUDIT_MANIFEST_NAME}: `{key}` entries must be strings"))?;
        validate_audit_path(path, globs)?;
        if !paths.insert(path.to_owned()) {
            bail!("{AUDIT_MANIFEST_NAME}: duplicate `{key}` entry `{path}`");
        }
    }
    Ok(paths.into_iter().collect())
}

fn validate_audit_path(path: &str, globs: bool) -> Result<()> {
    validate_project_relative_path(path)?;
    if !globs {
        return Ok(());
    }
    for component in path.split('/') {
        if component.contains(['?', '[', ']', '{', '}'])
            || (component.contains("**") && component != "**")
        {
            bail!("{AUDIT_MANIFEST_NAME}: unsupported glob syntax in `{path}`");
        }
    }
    Ok(())
}

fn validate_project_relative_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains(':')
        || path
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
    {
        bail!("{AUDIT_MANIFEST_NAME}: unsafe project-relative path `{path}`");
    }
    Ok(())
}

fn positive_limit(object: &serde_json::Map<String, Value>, key: &str) -> Result<Option<u64>> {
    object
        .get(key)
        .map(|value| {
            let number = value.as_u64().ok_or_else(|| {
                anyhow!("{AUDIT_MANIFEST_NAME}: `{key}` must be a positive integer")
            })?;
            checked_option_limit(number, key)
        })
        .transpose()
}

fn checked_option_limit(value: u64, key: &str) -> Result<u64> {
    if value == 0 {
        bail!("{key} must be greater than zero");
    }
    Ok(value)
}

fn audit_diagnostic(
    code: AssetDiagnosticCode,
    severity: AssetSeverity,
    path: &str,
    message: &str,
    measured: Option<u64>,
    limit: Option<u64>,
) -> AssetDiagnostic {
    AssetDiagnostic {
        code,
        severity,
        path: path.to_owned(),
        message: message.to_owned(),
        measured,
        limit,
    }
}

fn audit_glob_matches(pattern: &str, path: &str) -> bool {
    fn component_matches(pattern: &[u8], value: &[u8]) -> bool {
        let mut previous = vec![false; value.len() + 1];
        previous[0] = true;
        for &part in pattern {
            let mut next = vec![false; value.len() + 1];
            if part == b'*' {
                next[0] = previous[0];
                for index in 1..=value.len() {
                    next[index] = previous[index] || next[index - 1];
                }
            } else {
                for index in 1..=value.len() {
                    next[index] = previous[index - 1] && part == value[index - 1];
                }
            }
            previous = next;
        }
        previous[value.len()]
    }
    fn matches(pattern: &[&str], path: &[&str]) -> bool {
        match pattern.split_first() {
            None => path.is_empty(),
            Some((&"**", rest)) => {
                matches(rest, path)
                    || path
                        .split_first()
                        .is_some_and(|(_, remaining)| matches(pattern, remaining))
            }
            Some((first, rest)) => path.split_first().is_some_and(|(value, remaining)| {
                component_matches(first.as_bytes(), value.as_bytes()) && matches(rest, remaining)
            }),
        }
    }
    matches(
        &pattern.split('/').collect::<Vec<_>>(),
        &path.split('/').collect::<Vec<_>>(),
    )
}

struct MediaProbe {
    extension_mismatch: bool,
    dimensions: Option<(u32, u32)>,
}

fn inspect_media(path: &Path, relative: &str) -> Option<Result<MediaProbe>> {
    let extension = Path::new(relative)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let recognized_extension = matches!(
        extension.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "wav" | "mp3" | "ogg" | "flac"
    );
    let bytes = (|| -> io::Result<Vec<u8>> {
        let file = fs::File::open(path)?;
        let mut bytes = Vec::new();
        file.take(256 * 1024).read_to_end(&mut bytes)?;
        Ok(bytes)
    })();
    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(error) => return recognized_extension.then_some(Err(error.into())),
    };
    let signature = if bytes.starts_with(&[0xff, 0xd8]) {
        let dimensions = (|| -> MediaDimensions {
            let file = fs::File::open(path).map_err(|_| "unreadable JPEG header")?;
            let length = file.metadata().map_err(|_| "unreadable JPEG header")?.len();
            jpeg_dimensions(&mut io::BufReader::new(file), length)
        })();
        Some((MediaFormat::Jpeg, dimensions))
    } else {
        media_signature(&bytes)
    };
    if !recognized_extension && signature.is_none() {
        return None;
    }
    Some(match signature {
        Some((format, dimensions)) => match dimensions {
            Ok(dimensions) => Ok(MediaProbe {
                extension_mismatch: !format.accepts_extension(&extension),
                dimensions,
            }),
            Err(message) => Err(anyhow!(message)),
        },
        None => Err(anyhow!("unrecognized media signature")),
    })
}

#[derive(Clone, Copy)]
enum MediaFormat {
    Png,
    Jpeg,
    Gif,
    Bmp,
    Webp,
    Wav,
    Mp3,
    Ogg,
    Flac,
}

impl MediaFormat {
    fn accepts_extension(self, extension: &str) -> bool {
        match self {
            Self::Png => extension == "png",
            Self::Jpeg => matches!(extension, "jpg" | "jpeg"),
            Self::Gif => extension == "gif",
            Self::Bmp => extension == "bmp",
            Self::Webp => extension == "webp",
            Self::Wav => extension == "wav",
            Self::Mp3 => extension == "mp3",
            Self::Ogg => extension == "ogg",
            Self::Flac => extension == "flac",
        }
    }
}

type MediaDimensions = std::result::Result<Option<(u32, u32)>, &'static str>;

fn media_signature(bytes: &[u8]) -> Option<(MediaFormat, MediaDimensions)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let dimensions = if bytes.len() >= 33
            && u32::from_be_bytes(bytes[8..12].try_into().unwrap()) == 13
            && &bytes[12..16] == b"IHDR"
        {
            let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
            let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
            valid_dimensions(width, height)
        } else {
            Err("truncated PNG header")
        };
        return Some((MediaFormat::Png, dimensions));
    }
    if bytes.starts_with(&[0xff, 0xd8]) {
        return Some((
            MediaFormat::Jpeg,
            jpeg_dimensions(&mut io::Cursor::new(bytes), bytes.len() as u64),
        ));
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        let dimensions = if bytes.len() >= 13 {
            valid_dimensions(
                u32::from(u16::from_le_bytes(bytes[6..8].try_into().unwrap())),
                u32::from(u16::from_le_bytes(bytes[8..10].try_into().unwrap())),
            )
        } else {
            Err("truncated GIF header")
        };
        return Some((MediaFormat::Gif, dimensions));
    }
    if bytes.starts_with(b"BM") {
        let dimensions = if bytes.len() >= 54
            && u32::from_le_bytes(bytes[14..18].try_into().unwrap()) >= 40
            && bytes.len().saturating_sub(14)
                >= u32::from_le_bytes(bytes[14..18].try_into().unwrap()) as usize
        {
            let width = i32::from_le_bytes(bytes[18..22].try_into().unwrap()).unsigned_abs();
            let height = i32::from_le_bytes(bytes[22..26].try_into().unwrap()).unsigned_abs();
            valid_dimensions(width, height)
        } else {
            Err("truncated BMP header")
        };
        return Some((MediaFormat::Bmp, dimensions));
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        let dimensions = if bytes.len() >= 30 && &bytes[12..16] == b"VP8X" {
            let width = 1
                + u32::from(bytes[24])
                + (u32::from(bytes[25]) << 8)
                + (u32::from(bytes[26]) << 16);
            let height = 1
                + u32::from(bytes[27])
                + (u32::from(bytes[28]) << 8)
                + (u32::from(bytes[29]) << 16);
            valid_dimensions(width, height)
        } else if bytes.len() >= 30
            && &bytes[12..16] == b"VP8 "
            && &bytes[23..26] == b"\x9d\x01\x2a"
        {
            let width = u16::from_le_bytes(bytes[26..28].try_into().unwrap()) & 0x3fff;
            let height = u16::from_le_bytes(bytes[28..30].try_into().unwrap()) & 0x3fff;
            valid_dimensions(u32::from(width), u32::from(height))
        } else if bytes.len() >= 25 && &bytes[12..16] == b"VP8L" && bytes[20] == 0x2f {
            let width = 1 + u32::from(bytes[21]) + (u32::from(bytes[22] & 0x3f) << 8);
            let height = 1
                + u32::from(bytes[22] >> 6)
                + (u32::from(bytes[23]) << 2)
                + (u32::from(bytes[24] & 0x0f) << 10);
            valid_dimensions(width, height)
        } else {
            Err("unsupported or truncated WebP header")
        };
        return Some((MediaFormat::Webp, dimensions));
    }
    if bytes.starts_with(b"RIFF") && bytes.len() >= 12 && &bytes[8..12] == b"WAVE" {
        let dimensions = if bytes.len() >= 36 && &bytes[12..16] == b"fmt " {
            Ok(None)
        } else {
            Err("truncated WAVE header")
        };
        return Some((MediaFormat::Wav, dimensions));
    }
    if bytes.starts_with(b"OggS") {
        return Some((
            MediaFormat::Ogg,
            (bytes.len() >= 27)
                .then_some(None)
                .ok_or("truncated Ogg header"),
        ));
    }
    if bytes.starts_with(b"fLaC") {
        return Some((
            MediaFormat::Flac,
            (bytes.len() >= 8)
                .then_some(None)
                .ok_or("truncated FLAC header"),
        ));
    }
    if bytes.starts_with(b"ID3")
        || bytes.starts_with(&[0xff, 0xfb])
        || bytes.starts_with(&[0xff, 0xf3])
    {
        return Some((
            MediaFormat::Mp3,
            (bytes.len() >= 10)
                .then_some(None)
                .ok_or("truncated MP3 header"),
        ));
    }
    None
}

fn valid_dimensions(width: u32, height: u32) -> MediaDimensions {
    if width == 0 || height == 0 {
        Err("image dimensions must be nonzero")
    } else {
        Ok(Some((width, height)))
    }
}

fn jpeg_dimensions<R: Read + Seek>(reader: &mut R, file_length: u64) -> MediaDimensions {
    const MAX_HEADER_BYTES: u64 = 64 * 1024 * 1024;
    const MAX_SEGMENTS: usize = 4096;
    const MAX_MARKER_FILL: usize = 1024;
    let mut soi = [0_u8; 2];
    reader
        .read_exact(&mut soi)
        .map_err(|_| "truncated JPEG header")?;
    if soi != [0xff, 0xd8] {
        return Err("invalid JPEG signature");
    }
    for _ in 0..MAX_SEGMENTS {
        if reader
            .stream_position()
            .map_err(|_| "unreadable JPEG header")?
            >= MAX_HEADER_BYTES
        {
            return Ok(None);
        }
        let mut marker = [0_u8; 1];
        reader
            .read_exact(&mut marker)
            .map_err(|_| "truncated JPEG marker")?;
        if marker[0] != 0xff {
            return Err("invalid JPEG marker");
        }
        for _ in 0..MAX_MARKER_FILL {
            reader
                .read_exact(&mut marker)
                .map_err(|_| "truncated JPEG marker")?;
            if marker[0] != 0xff {
                break;
            }
        }
        if marker[0] == 0xff {
            return Ok(None);
        }
        match marker[0] {
            0x00 | 0xd8 => return Err("invalid JPEG marker"),
            0xd9 | 0xda => return Err("JPEG dimensions not found in header"),
            0x01 | 0xd0..=0xd7 => continue,
            _ => {}
        }
        let mut size = [0_u8; 2];
        reader
            .read_exact(&mut size)
            .map_err(|_| "truncated JPEG segment")?;
        let size = u64::from(u16::from_be_bytes(size));
        if size < 2 {
            return Err("invalid JPEG segment length");
        }
        let start = reader
            .stream_position()
            .map_err(|_| "unreadable JPEG header")?;
        let end = start + size - 2;
        if end > file_length {
            return Err("truncated JPEG segment");
        }
        if end > MAX_HEADER_BYTES {
            return Ok(None);
        }
        if matches!(marker[0], 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            if size < 8 {
                return Err("truncated JPEG dimensions");
            }
            let mut frame = [0_u8; 6];
            reader
                .read_exact(&mut frame)
                .map_err(|_| "truncated JPEG dimensions")?;
            if frame[5] == 0 || size < 8 + 3 * u64::from(frame[5]) {
                return Err("invalid JPEG frame header");
            }
            let height = u32::from(u16::from_be_bytes([frame[1], frame[2]]));
            let width = u32::from(u16::from_be_bytes([frame[3], frame[4]]));
            return valid_dimensions(width, height);
        }
        reader
            .seek(SeekFrom::Start(end))
            .map_err(|_| "unreadable JPEG header")?;
    }
    Ok(None)
}

pub fn validate_project_assets_with_options(
    project_root: &Path,
    options: &AssetValidationOptions,
) -> Result<AssetValidationReport> {
    let root = canonical_root(project_root)?;
    let settings = load_asset_validation_settings(&root, options)?;
    let (assets, mut referenced) = collect_project_assets_with_references(&root)?;
    let mut summary = AssetSummary::default();
    let mut watch_directories = BTreeSet::new();
    let mut diagnostics = Vec::new();
    let mut audited_bytes = 0_u64;
    let mut audited_pixels = 0_u64;

    for relative in &settings.dynamic_paths {
        let source = root.join(AUDIT_MANIFEST_NAME);
        match resolve_exact_disk_case(&root, relative, &source) {
            Ok(_) => {
                resolve_project_reference(&root, relative, &source)?;
                referenced.insert(relative.clone());
            }
            Err(error) if error.to_string().contains("does not exist") => {
                diagnostics.push(audit_diagnostic(
                    AssetDiagnosticCode::DeclaredDynamicPathMissing,
                    AssetSeverity::Error,
                    relative,
                    "declared dynamic asset path does not exist",
                    None,
                    None,
                ));
            }
            Err(error) => return Err(error),
        }
    }

    for (relative, path) in &assets {
        let metadata =
            fs::metadata(path).with_context(|| format!("could not inspect asset `{relative}`"))?;
        summary.files += 1;
        summary.bytes = summary.bytes.saturating_add(metadata.len());
        if let Some(parent) = path.parent() {
            watch_directories.insert(parent.to_path_buf());
        }
        let ignored = settings
            .ignored_paths
            .iter()
            .any(|pattern| audit_glob_matches(pattern, relative));
        if !ignored {
            audited_bytes = audited_bytes.saturating_add(metadata.len());
            if !referenced.contains(relative)
                && relative
                    .split('/')
                    .next()
                    .is_some_and(|root| ASSET_ROOTS.contains(&root))
            {
                let severity = match settings.orphan_policy {
                    OrphanPolicy::Ignore => None,
                    OrphanPolicy::Warn => Some(AssetSeverity::Warning),
                    OrphanPolicy::Error => Some(AssetSeverity::Error),
                };
                if let Some(severity) = severity {
                    diagnostics.push(audit_diagnostic(
                        AssetDiagnosticCode::OrphanAsset,
                        severity,
                        relative,
                        "asset has no known static or declared dynamic reference",
                        None,
                        None,
                    ));
                }
            }
            if let Some(limit) = settings.max_file_bytes
                && metadata.len() > limit
            {
                diagnostics.push(audit_diagnostic(
                    AssetDiagnosticCode::FileSizeLimit,
                    AssetSeverity::Error,
                    relative,
                    "asset exceeds file byte limit",
                    Some(metadata.len()),
                    Some(limit),
                ));
            }
        }
        if let Some(media) = inspect_media(path, relative) {
            match media {
                Ok(probe) => {
                    if probe.extension_mismatch {
                        diagnostics.push(audit_diagnostic(
                            AssetDiagnosticCode::ExtensionMismatch,
                            AssetSeverity::Warning,
                            relative,
                            "media signature does not match file extension",
                            None,
                            None,
                        ));
                    }
                    if let Some((width, height)) = probe.dimensions {
                        if !ignored {
                            audited_pixels = audited_pixels
                                .saturating_add(u64::from(width).saturating_mul(u64::from(height)));
                        }
                        if let Some(limit) = settings.max_image_dimension.filter(|_| !ignored) {
                            let maximum = width.max(height);
                            if maximum > limit {
                                diagnostics.push(audit_diagnostic(
                                    AssetDiagnosticCode::ImageDimensionLimit,
                                    AssetSeverity::Error,
                                    relative,
                                    "image dimension exceeds limit",
                                    Some(u64::from(maximum)),
                                    Some(u64::from(limit)),
                                ));
                            }
                        }
                    }
                }
                Err(_) => diagnostics.push(audit_diagnostic(
                    AssetDiagnosticCode::InvalidMediaHeader,
                    AssetSeverity::Warning,
                    relative,
                    "media header is invalid or unreadable",
                    None,
                    None,
                )),
            }
        }
    }
    if let Some(limit) = settings.max_total_bytes
        && audited_bytes > limit
    {
        diagnostics.push(audit_diagnostic(
            AssetDiagnosticCode::TotalSizeLimit,
            AssetSeverity::Error,
            "",
            "audited assets exceed total byte limit",
            Some(audited_bytes),
            Some(limit),
        ));
    }
    if let Some(limit) = settings.max_total_image_pixels
        && audited_pixels > limit
    {
        diagnostics.push(audit_diagnostic(
            AssetDiagnosticCode::TotalImagePixelsLimit,
            AssetSeverity::Error,
            "",
            "audited images exceed total pixel limit",
            Some(audited_pixels),
            Some(limit),
        ));
    }
    summary.watch_directories = watch_directories.into_iter().collect();
    AssetValidationReport::new(summary, diagnostics)
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PackSummary {
    pub files: usize,
    pub bytes: u64,
}

#[derive(Deserialize, Serialize)]
struct AssetManifest {
    format: String,
    files: Vec<AssetEntry>,
}

#[derive(Deserialize, Serialize)]
struct AssetEntry {
    path: String,
    sha256: String,
    size: u64,
}

pub fn validate_project_assets(project_root: &Path) -> Result<AssetSummary> {
    let report =
        validate_project_assets_with_options(project_root, &AssetValidationOptions::default())?;
    if let Some(diagnostic) = report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.severity == AssetSeverity::Error)
    {
        bail!(
            "asset validation failed: {} at `{}`: {}",
            serde_json::to_string(&diagnostic.code)?,
            diagnostic.path,
            diagnostic.message
        );
    }
    Ok(report.summary)
}

pub fn pack_project(project_root: &Path, output_directory: &Path) -> Result<PackSummary> {
    pack_project_with_rename(project_root, output_directory, |source, destination| {
        fs::rename(source, destination)
    })
}

fn pack_project_with_rename<F>(
    project_root: &Path,
    output_directory: &Path,
    mut rename: F,
) -> Result<PackSummary>
where
    F: FnMut(&Path, &Path) -> io::Result<()>,
{
    let root = canonical_root(project_root)?;
    let assets = collect_project_assets(&root)?;
    let output_input = absolute_output_path(output_directory)?;
    reject_symlink_components(&output_input, "asset output")?;
    let mut output = canonicalize_existing_prefix(&output_input)?;
    reject_asset_root_output(&root, &output)?;
    if assets.contains_key(MANIFEST_NAME) {
        bail!("project asset path `{MANIFEST_NAME}` is reserved for the pack manifest");
    }
    let output_parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    fs::create_dir_all(&output_parent)
        .with_context(|| format!("could not create output parent {}", output_parent.display()))?;
    reject_symlink_components(&output, "asset output")?;
    output = canonicalize_existing_prefix(&output)?;
    reject_asset_root_output(&root, &output)?;
    let output_exists = validate_pack_output_directory(&output)?;
    let old_manifest = if output_exists {
        read_existing_manifest(&output)?
    } else {
        None
    };
    let old_paths = manifest_paths(old_manifest.as_ref())?;
    let new_paths = assets.keys().cloned().collect::<BTreeSet<_>>();
    preflight_pack_output(&output, &old_paths, &new_paths)?;

    let stage = PackStage::create(&output_parent)?;
    let staged_files = stage.0.join("next");
    fs::create_dir(&staged_files).with_context(|| {
        format!(
            "could not create staging directory {}",
            staged_files.display()
        )
    })?;

    let mut entries = Vec::with_capacity(assets.len());
    let mut summary = PackSummary::default();
    for (relative, source) in assets {
        let bytes =
            fs::read(&source).with_context(|| format!("could not read asset `{relative}`"))?;
        let digest = Sha256::digest(&bytes);
        let sha256 = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let destination = staged_files.join(Path::new(&relative));
        let parent = destination
            .parent()
            .context("staged asset path has no parent directory")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("could not create staging directory {}", parent.display()))?;
        fs::write(&destination, &bytes)
            .with_context(|| format!("could not stage asset `{relative}`"))?;
        summary.files += 1;
        summary.bytes = summary.bytes.saturating_add(bytes.len() as u64);
        entries.push(AssetEntry {
            path: relative,
            sha256,
            size: bytes.len() as u64,
        });
    }
    let manifest = AssetManifest {
        format: MANIFEST_FORMAT.to_owned(),
        files: entries,
    };
    let mut manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    manifest_bytes.push(b'\n');
    fs::write(staged_files.join(MANIFEST_NAME), &manifest_bytes)
        .context("could not stage asset manifest")?;

    publish_staged_pack(
        &output,
        &stage.0,
        &staged_files,
        &old_paths,
        &new_paths,
        output_exists,
        &mut rename,
    )?;
    Ok(summary)
}

struct PackStage(PathBuf);

impl PackStage {
    fn create(parent: &Path) -> Result<Self> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        for attempt in 0_u32..u32::MAX {
            let candidate = parent.join(format!(
                ".bornengine-assets-{}-{timestamp}-{attempt}",
                std::process::id()
            ));
            match fs::create_dir(&candidate) {
                Ok(()) => return Ok(Self(candidate)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "could not create pack staging directory {}",
                            candidate.display()
                        )
                    });
                }
            }
        }
        bail!("could not allocate a unique asset pack staging directory")
    }
}

impl Drop for PackStage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn validate_pack_output_directory(output: &Path) -> Result<bool> {
    match fs::symlink_metadata(output) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!(
                "refusing to use symbolic link as asset output: {}",
                output.display()
            );
        }
        Ok(metadata) if metadata.is_dir() => Ok(true),
        Ok(_) => bail!(
            "asset output exists and is not a directory: {}",
            output.display()
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("could not inspect {}", output.display())),
    }
}

fn manifest_paths(manifest: Option<&AssetManifest>) -> Result<BTreeSet<String>> {
    let mut paths = BTreeSet::new();
    if let Some(manifest) = manifest {
        for entry in &manifest.files {
            let path = normalize_pack_path(&entry.path)?;
            if path == MANIFEST_NAME {
                bail!("asset manifest must not list itself as a packed file");
            }
            if !paths.insert(path.clone()) {
                bail!("asset manifest contains duplicate path `{path}`");
            }
        }
    }
    Ok(paths)
}

fn preflight_pack_output(
    output: &Path,
    old_paths: &BTreeSet<String>,
    new_paths: &BTreeSet<String>,
) -> Result<()> {
    for relative in old_paths.union(new_paths) {
        let path = output.join(relative);
        reject_output_parent_symlinks(output, &path)?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!("refusing to replace symbolic link packed asset `{relative}`");
            }
            Ok(metadata) if !metadata.is_file() => {
                bail!("packed asset path `{relative}` is not a regular file");
            }
            Ok(_) if new_paths.contains(relative) && !old_paths.contains(relative) => {
                bail!("refusing to replace untracked output file `{relative}`");
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("could not inspect packed asset `{relative}`"));
            }
        }
    }
    Ok(())
}

fn publish_staged_pack<F>(
    output: &Path,
    stage_root: &Path,
    staged_files: &Path,
    old_paths: &BTreeSet<String>,
    new_paths: &BTreeSet<String>,
    output_exists: bool,
    rename: &mut F,
) -> Result<()>
where
    F: FnMut(&Path, &Path) -> io::Result<()>,
{
    let mut output_created = false;
    if !output_exists {
        fs::create_dir(output)
            .with_context(|| format!("could not create asset output {}", output.display()))?;
        output_created = true;
    }

    let mut backup_paths = old_paths.clone();
    backup_paths.insert(MANIFEST_NAME.to_owned());
    let mut moved_to_backup = Vec::<(PathBuf, PathBuf)>::new();
    let mut published = Vec::<PathBuf>::new();
    let mut created_directories = Vec::<PathBuf>::new();

    let publish_result = (|| -> Result<()> {
        for relative in &backup_paths {
            let destination = output.join(relative);
            match fs::symlink_metadata(&destination) {
                Ok(_) => {
                    let backup = stage_root.join("previous").join(relative);
                    let backup_parent = backup
                        .parent()
                        .context("asset backup path has no parent directory")?;
                    fs::create_dir_all(backup_parent).with_context(|| {
                        format!(
                            "could not create backup directory {}",
                            backup_parent.display()
                        )
                    })?;
                    rename(&destination, &backup).with_context(|| {
                        format!("could not back up existing asset {}", destination.display())
                    })?;
                    moved_to_backup.push((backup, destination));
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("could not inspect existing asset {}", destination.display())
                    });
                }
            }
        }

        for relative in new_paths {
            let source = staged_files.join(relative);
            let destination = output.join(relative);
            create_safe_parent_directories(
                output,
                destination.parent().unwrap_or(output),
                &mut created_directories,
            )?;
            rename(&source, &destination)
                .with_context(|| format!("could not publish packed asset `{relative}`"))?;
            published.push(destination);
        }

        let source_manifest = staged_files.join(MANIFEST_NAME);
        let destination_manifest = output.join(MANIFEST_NAME);
        rename(&source_manifest, &destination_manifest)
            .context("could not publish asset manifest")?;
        published.push(destination_manifest);
        Ok(())
    })();

    if let Err(error) = publish_result {
        let rollback_errors = rollback_pack(
            output,
            &moved_to_backup,
            &published,
            &created_directories,
            output_created,
            rename,
        );
        if rollback_errors.is_empty() {
            return Err(error).context("asset pack failed; the previous output was restored");
        }
        bail!(
            "asset pack failed: {error:#}; rollback also failed: {}",
            rollback_errors.join("; ")
        );
    }
    Ok(())
}

fn rollback_pack<F>(
    output: &Path,
    moved_to_backup: &[(PathBuf, PathBuf)],
    published: &[PathBuf],
    created_directories: &[PathBuf],
    output_created: bool,
    rename: &mut F,
) -> Vec<String>
where
    F: FnMut(&Path, &Path) -> io::Result<()>,
{
    let mut errors = Vec::new();
    for path in published.iter().rev() {
        if let Err(error) = fs::remove_file(path) {
            if error.kind() != io::ErrorKind::NotFound {
                errors.push(format!("could not remove {}: {error}", path.display()));
            }
        }
    }
    for (backup, destination) in moved_to_backup.iter().rev() {
        if let Some(parent) = destination.parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            errors.push(format!(
                "could not restore {}: {error}",
                destination.display()
            ));
            continue;
        }
        if let Err(error) = rename(backup, destination) {
            errors.push(format!(
                "could not restore {}: {error}",
                destination.display()
            ));
        }
    }
    for directory in created_directories.iter().rev() {
        if let Err(error) = fs::remove_dir(directory)
            && error.kind() != io::ErrorKind::NotFound
            && error.kind() != io::ErrorKind::DirectoryNotEmpty
        {
            errors.push(format!("could not remove {}: {error}", directory.display()));
        }
    }
    if output_created
        && let Err(error) = fs::remove_dir(output)
        && error.kind() != io::ErrorKind::NotFound
        && error.kind() != io::ErrorKind::DirectoryNotEmpty
    {
        errors.push(format!("could not remove {}: {error}", output.display()));
    }
    errors
}

fn read_existing_manifest(output: &Path) -> Result<Option<AssetManifest>> {
    let path = output.join(MANIFEST_NAME);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("could not inspect {}", path.display())),
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            bail!(
                "refusing to read non-regular asset manifest {}",
                path.display()
            );
        }
        Ok(_) => {
            let contents = fs::read(&path)
                .with_context(|| format!("could not read asset manifest {}", path.display()))?;
            let manifest: AssetManifest = serde_json::from_slice(&contents)
                .with_context(|| format!("invalid asset manifest {}", path.display()))?;
            if manifest.format != MANIFEST_FORMAT {
                bail!("unrecognized asset manifest format in {}", path.display());
            }
            Ok(Some(manifest))
        }
    }
}

fn normalize_pack_path(value: &str) -> Result<String> {
    if value.is_empty() || value.contains('\\') || value.starts_with('/') || value.contains(':') {
        bail!("asset manifest contains an unsafe path: `{value}`");
    }
    let path = Path::new(value);
    if path.is_absolute()
        || path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("asset manifest contains an unsafe path: `{value}`");
    }
    Ok(path.to_string_lossy().replace('\\', "/"))
}

fn reject_output_parent_symlinks(output: &Path, path: &Path) -> Result<()> {
    let parent = path.parent().unwrap_or(output);
    let relative = parent
        .strip_prefix(output)
        .context("stale asset path escaped output directory")?;
    let mut current = output.to_path_buf();
    for component in relative.components() {
        let Component::Normal(value) = component else {
            bail!("unsafe packed output path");
        };
        current.push(value);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                bail!("refusing to follow non-directory {}", current.display());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("could not inspect {}", current.display()));
            }
        }
    }
    Ok(())
}

fn canonical_root(path: &Path) -> Result<PathBuf> {
    let root = path
        .canonicalize()
        .with_context(|| format!("project directory does not exist: {}", path.display()))?;
    if !root.is_dir() {
        bail!("project root is not a directory: {}", root.display());
    }
    Ok(root)
}

fn reject_asset_root_output(root: &Path, output: &Path) -> Result<()> {
    for source_root in ASSET_ROOTS {
        if output.starts_with(root.join(source_root)) {
            bail!(
                "asset output {} is inside the project asset root `{source_root}`; choose a build or distribution directory",
                output.display()
            );
        }
    }
    Ok(())
}

fn collect_project_assets(root: &Path) -> Result<BTreeMap<String, PathBuf>> {
    collect_project_assets_with_references(root).map(|(files, _)| files)
}

fn collect_project_assets_with_references(
    root: &Path,
) -> Result<(BTreeMap<String, PathBuf>, BTreeSet<String>)> {
    let mut files = BTreeMap::new();
    let mut spelling_by_normalized = BTreeMap::new();
    let mut referenced = BTreeSet::new();
    for directory in ASSET_ROOTS {
        let path = root.join(directory);
        match fs::symlink_metadata(&path) {
            Ok(_) => collect_asset_tree(
                root,
                &path,
                &mut files,
                &mut spelling_by_normalized,
                &mut BTreeSet::new(),
            )?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| format!("could not inspect {}", path.display()));
            }
        }
    }
    collect_world_documents(
        root,
        root,
        &mut files,
        &mut spelling_by_normalized,
        &mut referenced,
    )?;
    Ok((files, referenced))
}

fn collect_asset_tree(
    root: &Path,
    directory: &Path,
    files: &mut BTreeMap<String, PathBuf>,
    spelling_by_normalized: &mut BTreeMap<String, String>,
    ancestors: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    let metadata = fs::symlink_metadata(directory)
        .with_context(|| format!("could not inspect {}", directory.display()))?;
    if metadata.file_type().is_symlink() {
        let target = directory
            .canonicalize()
            .with_context(|| format!("could not resolve symbolic link {}", directory.display()))?;
        ensure_inside_root(root, &target, "asset directory")?;
        if target.is_dir() {
            if !ancestors.insert(target.clone()) {
                return Ok(());
            }
            for entry in fs::read_dir(directory)
                .with_context(|| format!("could not read {}", directory.display()))?
            {
                collect_asset_tree(
                    root,
                    &entry?.path(),
                    files,
                    spelling_by_normalized,
                    ancestors,
                )?;
            }
            ancestors.remove(&target);
            return Ok(());
        }
        add_asset(root, directory, files, spelling_by_normalized)?;
        return Ok(());
    }
    if metadata.is_file() {
        add_asset(root, directory, files, spelling_by_normalized)?;
        return Ok(());
    }
    if !metadata.is_dir() {
        bail!("unsupported asset file type: {}", directory.display());
    }
    let canonical = directory
        .canonicalize()
        .with_context(|| format!("could not resolve {}", directory.display()))?;
    ensure_inside_root(root, &canonical, "asset directory")?;
    if !ancestors.insert(canonical.clone()) {
        return Ok(());
    }
    for entry in fs::read_dir(directory)
        .with_context(|| format!("could not read {}", directory.display()))?
    {
        let path = entry?.path();
        collect_asset_tree(root, &path, files, spelling_by_normalized, ancestors)?;
    }
    ancestors.remove(&canonical);
    Ok(())
}

fn collect_world_documents(
    root: &Path,
    directory: &Path,
    files: &mut BTreeMap<String, PathBuf>,
    spelling_by_normalized: &mut BTreeMap<String, String>,
    referenced: &mut BTreeSet<String>,
) -> Result<()> {
    let metadata = fs::symlink_metadata(directory)
        .with_context(|| format!("could not inspect {}", directory.display()))?;
    if metadata.file_type().is_symlink() {
        let target = directory
            .canonicalize()
            .with_context(|| format!("could not resolve symbolic link {}", directory.display()))?;
        ensure_inside_root(root, &target, "project file")?;
        if target.is_dir() {
            return Ok(());
        }
        if is_world_document(directory) {
            collect_world_document(root, directory, files, spelling_by_normalized, referenced)?;
        }
        return Ok(());
    }
    if metadata.is_file() {
        if is_world_document(directory) {
            collect_world_document(root, directory, files, spelling_by_normalized, referenced)?;
        }
        return Ok(());
    }
    if !metadata.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(directory)
        .with_context(|| format!("could not read {}", directory.display()))?
    {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if directory == root && SKIPPED_DIRECTORIES.contains(&name) {
            continue;
        }
        collect_world_documents(root, &path, files, spelling_by_normalized, referenced)?;
    }
    Ok(())
}

fn collect_world_document(
    root: &Path,
    path: &Path,
    files: &mut BTreeMap<String, PathBuf>,
    spelling_by_normalized: &mut BTreeMap<String, String>,
    referenced: &mut BTreeSet<String>,
) -> Result<()> {
    let bytes = fs::read(path)
        .with_context(|| format!("could not read world document {}", path.display()))?;
    let world: Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("{}: invalid BornEngine world JSON", path.display()))?;
    if world.get("format").and_then(Value::as_str) != Some(WORLD_FORMAT) {
        return Ok(());
    }
    let version = world.get("version").and_then(Value::as_u64);
    if version != Some(WORLD_VERSION) {
        bail!(
            "{}: unsupported world format version {:?}",
            path.display(),
            version
        );
    }
    let listed_assets = world
        .get("assets")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("{}: `assets` must be an array", path.display()))?;
    let mut seen = BTreeSet::new();
    for value in listed_assets {
        let raw = value
            .as_str()
            .ok_or_else(|| anyhow!("{}: every asset path must be a string", path.display()))?;
        let relative = normalize_reference(raw, path)?;
        if !seen.insert(relative.clone()) {
            bail!(
                "{}: duplicate normalized asset path `{relative}`",
                path.display()
            );
        }
        let resolved = resolve_project_reference(root, &relative, path)?;
        add_normalized_asset(root, &relative, resolved, files, spelling_by_normalized)?;
        referenced.insert(relative);
    }
    Ok(())
}

fn add_asset(
    root: &Path,
    path: &Path,
    files: &mut BTreeMap<String, PathBuf>,
    spelling_by_normalized: &mut BTreeMap<String, String>,
) -> Result<()> {
    let canonical = path
        .canonicalize()
        .with_context(|| format!("could not resolve project asset {}", path.display()))?;
    ensure_inside_root(root, &canonical, "asset")?;
    if !canonical.is_file() {
        return Ok(());
    }
    let relative = path
        .strip_prefix(root)
        .context("asset escaped project root")?
        .to_string_lossy()
        .replace('\\', "/");
    add_normalized_asset(root, &relative, canonical, files, spelling_by_normalized)
}

fn add_normalized_asset(
    _root: &Path,
    normalized: &str,
    path: PathBuf,
    files: &mut BTreeMap<String, PathBuf>,
    spelling_by_normalized: &mut BTreeMap<String, String>,
) -> Result<()> {
    if let Some(previous) = spelling_by_normalized.get(normalized) {
        if previous != normalized {
            bail!("asset paths `{previous}` and `{normalized}` normalize to the same project path");
        }
        files.entry(normalized.to_owned()).or_insert(path);
        return Ok(());
    }
    spelling_by_normalized.insert(normalized.to_owned(), normalized.to_owned());
    files.insert(normalized.to_owned(), path);
    Ok(())
}

fn normalize_reference(raw: &str, world_file: &Path) -> Result<String> {
    if raw.is_empty() || raw.contains('\\') || raw.starts_with('/') || raw.contains(':') {
        bail!("{}: unsafe asset path `{raw}`", world_file.display());
    }
    let path = Path::new(raw);
    if path.is_absolute()
        || path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("{}: unsafe asset path `{raw}`", world_file.display());
    }
    let normalized = path.to_string_lossy().replace('\\', "/");
    if normalized
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        bail!("{}: unsafe asset path `{raw}`", world_file.display());
    }
    Ok(normalized)
}

fn resolve_project_reference(root: &Path, relative: &str, world_file: &Path) -> Result<PathBuf> {
    let path = resolve_exact_disk_case(root, relative, world_file)?;
    let canonical = path.canonicalize().with_context(|| {
        format!(
            "{}: could not resolve asset `{relative}`",
            world_file.display()
        )
    })?;
    ensure_inside_root(
        root,
        &canonical,
        &format!("asset `{relative}` in {}", world_file.display()),
    )?;
    if !canonical.is_file() {
        bail!(
            "{}: referenced asset `{relative}` is not a file",
            world_file.display()
        );
    }
    Ok(canonical)
}

fn resolve_exact_disk_case(root: &Path, relative: &str, world_file: &Path) -> Result<PathBuf> {
    let mut current = root.to_path_buf();
    let mut actual_parts = Vec::new();
    let mut case_mismatch = false;
    for part in relative.split('/') {
        let entries = match fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                bail!(
                    "{}: referenced asset `{relative}` does not exist",
                    world_file.display()
                );
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "{}: could not inspect referenced asset `{relative}`",
                        world_file.display()
                    )
                });
            }
        };
        let mut exact = None;
        let mut case_insensitive = None;
        for entry in entries {
            let entry = entry.with_context(|| {
                format!(
                    "{}: could not inspect referenced asset `{relative}`",
                    world_file.display()
                )
            })?;
            let name = entry.file_name();
            if name == std::ffi::OsStr::new(part) {
                exact = Some(name);
                break;
            }
            if case_insensitive.is_none() && name.to_string_lossy().eq_ignore_ascii_case(part) {
                case_insensitive = Some(name);
            }
        }
        let name = if let Some(exact) = exact {
            exact
        } else if let Some(case_insensitive) = case_insensitive {
            case_mismatch = true;
            case_insensitive
        } else {
            bail!(
                "{}: referenced asset `{relative}` does not exist",
                world_file.display()
            );
        };
        actual_parts.push(name.to_string_lossy().into_owned());
        current.push(name);
    }
    if case_mismatch {
        bail!(
            "{}: referenced asset `{relative}` does not match disk case; did you mean `{}`?",
            world_file.display(),
            actual_parts.join("/")
        );
    }
    Ok(current)
}

fn ensure_inside_root(root: &Path, path: &Path, label: &str) -> Result<()> {
    if !path.starts_with(root) {
        bail!(
            "{label} resolves outside the project root: {}",
            path.display()
        );
    }
    Ok(())
}

fn is_world_document(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".world2d.json"))
}

fn absolute_output_path(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if normalized.file_name().is_some() {
                    normalized.pop();
                }
            }
            Component::Normal(value) => normalized.push(value),
        }
    }
    Ok(normalized)
}

fn canonicalize_existing_prefix(path: &Path) -> Result<PathBuf> {
    let mut current = absolute_output_path(path)?;
    let mut missing_suffix = Vec::<OsString>::new();
    loop {
        match fs::canonicalize(&current) {
            Ok(mut canonical) => {
                for component in missing_suffix.iter().rev() {
                    canonical.push(component);
                }
                return Ok(canonical);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let name = current.file_name().ok_or_else(|| {
                    anyhow!("could not resolve asset output path: {}", path.display())
                })?;
                missing_suffix.push(name.to_os_string());
                current = current
                    .parent()
                    .ok_or_else(|| {
                        anyhow!("could not resolve asset output path: {}", path.display())
                    })?
                    .to_path_buf();
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("could not resolve asset output path {}", path.display())
                });
            }
        }
    }
}

fn reject_symlink_components(path: &Path, label: &str) -> Result<()> {
    let absolute = absolute_output_path(path)?;
    let mut current = PathBuf::new();
    for component in absolute.components() {
        current.push(component.as_os_str());
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata)
                if metadata.file_type().is_symlink() && !is_standard_macos_path_alias(&current) =>
            {
                bail!(
                    "refusing to use symbolic link in {label} path: {}",
                    current.display()
                );
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("could not inspect {}", current.display()));
            }
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn is_standard_macos_path_alias(path: &Path) -> bool {
    [
        (Path::new("/var"), Path::new("/private/var")),
        (Path::new("/tmp"), Path::new("/private/tmp")),
    ]
    .iter()
    .any(|(alias, target)| {
        path == *alias && fs::canonicalize(alias).is_ok_and(|resolved| resolved == *target)
    })
}

#[cfg(not(target_os = "macos"))]
fn is_standard_macos_path_alias(_path: &Path) -> bool {
    false
}

fn create_safe_parent_directories(
    root: &Path,
    parent: &Path,
    created_directories: &mut Vec<PathBuf>,
) -> Result<()> {
    if !parent.starts_with(root) {
        bail!("packed asset path escaped output directory");
    }
    let relative = parent.strip_prefix(root).unwrap_or(Path::new(""));
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            bail!("unsafe packed output path")
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                bail!(
                    "refusing to write packed asset through non-directory {}",
                    current.display()
                );
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current)
                    .with_context(|| format!("could not create {}", current.display()))?;
                created_directories.push(current.clone());
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("could not inspect {}", current.display()));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{jpeg_dimensions, media_signature, pack_project, pack_project_with_rename};
    use std::fs;

    #[test]
    fn webp_lossy_and_lossless_headers_report_dimensions() {
        let mut lossy = b"RIFF\x16\0\0\0WEBPVP8 \x0a\0\0\0\0\0\0\x9d\x01\x2a\x04\0\x02\0".to_vec();
        let mut lossless = b"RIFF\x11\0\0\0WEBPVP8L\x05\0\0\0\x2f\x03\x80\0\0".to_vec();
        assert_eq!(media_signature(&lossy).unwrap().1.unwrap(), Some((4, 2)));
        assert_eq!(media_signature(&lossless).unwrap().1.unwrap(), Some((4, 3)));
        lossy.truncate(25);
        lossless.truncate(23);
        assert!(media_signature(&lossy).unwrap().1.is_err());
        assert!(media_signature(&lossless).unwrap().1.is_err());
    }

    #[test]
    fn image_headers_truncated_after_dimensions_are_invalid() {
        let png =
            &include_bytes!("../../tests/fixtures/asset_audit/complete/assets/referenced.png")
                [..24];
        let gif = b"GIF89a\x01\0\x01\0";
        let mut bmp = vec![0_u8; 26];
        bmp[..2].copy_from_slice(b"BM");
        bmp[14..18].copy_from_slice(&40_u32.to_le_bytes());
        bmp[18..22].copy_from_slice(&1_i32.to_le_bytes());
        bmp[22..26].copy_from_slice(&1_i32.to_le_bytes());
        for bytes in [png, gif.as_slice(), bmp.as_slice()] {
            assert!(media_signature(bytes).unwrap().1.is_err());
        }
    }

    #[test]
    fn jpeg_segment_scan_cap_is_inconclusive_instead_of_invalid() {
        let mut bytes = vec![0xff, 0xd8];
        for _ in 0..4097 {
            bytes.extend_from_slice(&[0xff, 0xe1, 0x00, 0x02]);
        }
        bytes.extend_from_slice(
            &include_bytes!("../../tests/fixtures/asset_audit/jpeg-long-app.jpg")[2..],
        );
        let result = jpeg_dimensions(&mut std::io::Cursor::new(&bytes), bytes.len() as u64);
        assert_eq!(result, Ok(None));
    }

    #[test]
    fn failed_publish_rolls_back_files_and_manifest() {
        let project = tempfile::tempdir().unwrap();
        fs::create_dir_all(project.path().join("assets")).unwrap();
        fs::write(project.path().join("assets/a.bin"), b"old-a").unwrap();
        fs::write(project.path().join("assets/b.bin"), b"old-b").unwrap();
        let output = tempfile::tempdir().unwrap();
        pack_project(project.path(), output.path()).unwrap();
        fs::write(output.path().join("notes.txt"), b"unrelated user file").unwrap();
        let previous_manifest = fs::read(output.path().join("assets.manifest.json")).unwrap();

        fs::write(project.path().join("assets/a.bin"), b"new-a").unwrap();
        fs::write(project.path().join("assets/b.bin"), b"new-b").unwrap();
        let mut rename_attempt = 0;
        let mut failed_once = false;
        let error =
            pack_project_with_rename(project.path(), output.path(), |source, destination| {
                rename_attempt += 1;
                if rename_attempt == 5 && !failed_once {
                    failed_once = true;
                    return Err(std::io::Error::other("injected publish failure"));
                }
                fs::rename(source, destination)
            })
            .unwrap_err()
            .to_string();

        assert!(error.contains("previous output was restored"), "{error}");
        assert!(failed_once);
        assert_eq!(
            fs::read(output.path().join("assets/a.bin")).unwrap(),
            b"old-a"
        );
        assert_eq!(
            fs::read(output.path().join("assets/b.bin")).unwrap(),
            b"old-b"
        );
        assert_eq!(
            fs::read(output.path().join("assets.manifest.json")).unwrap(),
            previous_manifest
        );
        assert_eq!(
            fs::read(output.path().join("notes.txt")).unwrap(),
            b"unrelated user file"
        );
    }
}
