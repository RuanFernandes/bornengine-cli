use anyhow::{Context, Result, bail};
use boa_ast::{
    ModuleItem,
    declaration::ExportDeclaration,
    expression::ImportCall,
    scope::Scope,
    visitor::{VisitWith, Visitor},
};
use boa_interner::Interner;
use boa_parser::{Parser, Source};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read};
use std::ops::ControlFlow;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MANIFEST_NAME: &str = "bornengine.script.json";
const MANIFEST_FORMAT: &str = "bornengine-script-v1";
const API_VERSION: u32 = 1;
const MAX_SOURCE_BYTES: u64 = 1024 * 1024;
const MAX_METADATA_BYTES: u64 = 64 * 1024;
const MAX_SOURCE_NESTING: usize = 128;
const MAX_SOURCE_RECURSIVE_STEPS: usize = 128;
const OWNERSHIP_NAME: &str = ".bornengine-pack.json";
const ALLOWED_PERMISSIONS: [&str; 4] = [
    "log",
    "self.particles.emit",
    "self.read",
    "self.transform.write",
];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ScriptPackageManifest {
    pub format: String,
    pub api_version: u32,
    pub entry: String,
    pub permissions: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScriptPackSummary {
    pub files: usize,
    pub bytes: u64,
    pub entry: String,
}

struct ScriptPackage {
    manifest: ScriptPackageManifest,
    root: PathBuf,
    entry_bytes: Vec<u8>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct PackOwnership {
    format: String,
    manifest_sha256: String,
    entry_sha256: String,
}

pub fn check_package(manifest_path: Option<&Path>) -> Result<ScriptPackageManifest> {
    let package = read_package(manifest_path)?;
    Ok(package.manifest)
}

pub fn pack_package(
    manifest_path: Option<&Path>,
    output_directory: &Path,
) -> Result<ScriptPackSummary> {
    let package = read_package(manifest_path)?;
    let entry_bytes = &package.entry_bytes;

    let requested_output = absolute_path(output_directory)?;
    let output_parent = requested_output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    fs::create_dir_all(&output_parent)
        .with_context(|| format!("could not create output parent {}", output_parent.display()))?;

    let output_parent = fs::canonicalize(&output_parent).with_context(|| {
        format!(
            "could not resolve output parent {}",
            output_parent.display()
        )
    })?;
    let output_name = requested_output
        .file_name()
        .filter(|name| !name.is_empty())
        .context("script output must name a directory")?;
    let output = output_parent.join(output_name);
    if output == package.root || package.root.starts_with(&output) {
        bail!("script output must not replace the package root or one of its parents");
    }

    let output_exists = validate_output_directory(&output)?;
    if output_exists {
        validate_previous_pack(&output)?;
    }

    let mut stage = ScriptPackStage::create(&output_parent)?;
    let staged_files = stage.path.join("next");
    fs::create_dir(&staged_files).with_context(|| {
        format!(
            "could not create staging directory {}",
            staged_files.display()
        )
    })?;
    let staged_entry = staged_files.join(Path::new(&package.manifest.entry));
    let staged_entry_parent = staged_entry
        .parent()
        .context("script entry path has no parent directory")?;
    fs::create_dir_all(staged_entry_parent).with_context(|| {
        format!(
            "could not create staging directory {}",
            staged_entry_parent.display()
        )
    })?;
    fs::write(&staged_entry, entry_bytes)
        .with_context(|| format!("could not stage script entry `{}`", package.manifest.entry))?;

    let mut manifest_bytes = serde_json::to_vec_pretty(&package.manifest)?;
    manifest_bytes.push(b'\n');
    let manifest_digest = sha256(&manifest_bytes);
    let manifest_bytes_len = manifest_bytes.len();
    fs::write(staged_files.join(MANIFEST_NAME), manifest_bytes)
        .context("could not stage script manifest")?;
    let marker = PackOwnership {
        format: "bornengine-cli-script-pack-v1".into(),
        manifest_sha256: manifest_digest,
        entry_sha256: sha256(entry_bytes),
    };
    let mut marker_bytes = serde_json::to_vec_pretty(&marker)?;
    marker_bytes.push(b'\n');
    let marker_bytes_len = marker_bytes.len();
    fs::write(staged_files.join(OWNERSHIP_NAME), marker_bytes)
        .context("could not stage script pack ownership marker")?;

    publish_pack(&output, &mut stage, &staged_files, output_exists)?;
    Ok(ScriptPackSummary {
        files: 3,
        bytes: (entry_bytes.len() + manifest_bytes_len + marker_bytes_len) as u64,
        entry: package.manifest.entry,
    })
}

fn read_package(manifest_path: Option<&Path>) -> Result<ScriptPackage> {
    let manifest_input = manifest_path.unwrap_or_else(|| Path::new(MANIFEST_NAME));
    let requested_manifest = absolute_path(manifest_input)?;
    let manifest_parent = requested_manifest
        .parent()
        .context("script manifest has no parent directory")?;
    let manifest_name = requested_manifest
        .file_name()
        .context("script manifest must name a file")?;
    let manifest_parent = fs::canonicalize(manifest_parent).with_context(|| {
        format!(
            "could not resolve script manifest parent {}",
            manifest_parent.display()
        )
    })?;
    let manifest_path = manifest_parent.join(manifest_name);
    let metadata = fs::symlink_metadata(&manifest_path).with_context(|| {
        format!(
            "could not inspect script manifest {}",
            manifest_path.display()
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!(
            "script manifest must be a regular file: {}",
            manifest_path.display()
        );
    }

    let manifest_path = fs::canonicalize(&manifest_path).with_context(|| {
        format!(
            "could not resolve script manifest {}",
            manifest_path.display()
        )
    })?;
    let root = manifest_path
        .parent()
        .context("script manifest has no package directory")?
        .to_path_buf();
    let bytes = read_bounded_metadata(&manifest_path, "script manifest")?;
    let manifest: ScriptPackageManifest = serde_json::from_slice(&bytes)
        .with_context(|| format!("invalid script manifest {}", manifest_path.display()))?;
    validate_manifest(&manifest)?;
    let entry_path = resolve_regular_entry(&root, &manifest.entry)?;
    let entry_bytes = read_bounded_entry(&entry_path, &manifest.entry)?;
    validate_source(&entry_bytes)?;
    Ok(ScriptPackage {
        manifest,
        root,
        entry_bytes,
    })
}

fn read_bounded_entry(path: &Path, entry: &str) -> Result<Vec<u8>> {
    let metadata =
        fs::metadata(path).with_context(|| format!("could not inspect script entry `{entry}`"))?;
    if metadata.len() > MAX_SOURCE_BYTES {
        bail!("script entry `{entry}` exceeds the 1048576 byte limit");
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .with_context(|| format!("could not read script entry `{entry}`"))?
        .take(MAX_SOURCE_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("could not read script entry `{entry}`"))?;
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        bail!("script entry `{entry}` exceeds the 1048576 byte limit");
    }
    Ok(bytes)
}

fn validate_source(bytes: &[u8]) -> Result<()> {
    let source = std::str::from_utf8(bytes).context("script entry must be UTF-8")?;
    validate_source_nesting(source)?;
    let mut parser = Parser::new(Source::from_bytes(source));
    let mut interner = Interner::default();
    let module = parser
        .parse_module(&Scope::new_global(), &mut interner)
        .map_err(|error| anyhow::anyhow!("script entry has invalid JavaScript syntax: {error}"))?;
    for item in module.items().items() {
        match item {
            ModuleItem::ImportDeclaration(_)
            | ModuleItem::ExportDeclaration(ExportDeclaration::ReExport { .. }) => {
                bail!(
                    "script entry uses a static import or re-export, but v1 has no module loader"
                );
            }
            _ => {}
        }
    }
    struct DynamicImportFinder;
    impl<'ast> Visitor<'ast> for DynamicImportFinder {
        type BreakTy = ();

        fn visit_import_call(&mut self, _: &'ast ImportCall) -> ControlFlow<Self::BreakTy> {
            ControlFlow::Break(())
        }
    }
    if module.visit_with(&mut DynamicImportFinder).is_break() {
        bail!("script entry uses dynamic import, but v1 has no module loader");
    }
    Ok(())
}

// Boa's parser and AST visitor recurse through nested expressions. Bound
// delimiter depth and recursive expression chains before either phase sees
// untrusted source. Boa still supplies the authoritative syntax checks.
fn validate_source_nesting(source: &str) -> Result<()> {
    #[derive(Clone, Copy)]
    enum Mode {
        Code,
        Single,
        Double,
        Template,
        Regex,
        RegexClass,
        LineComment,
        BlockComment,
    }
    struct Delimiter {
        kind: u8,
        outer_chain: usize,
    }
    let bytes = source.as_bytes();
    let mut mode = Mode::Code;
    let mut delimiters = Vec::new();
    let mut index = 0;
    let mut recursive_chain = 0;
    let mut expect_operand = true;
    let mut pending_condition = false;
    let mut member_dot = false;
    let mut can_start_object = false;
    while index < bytes.len() {
        let current = bytes[index];
        let next = bytes.get(index + 1).copied();
        if matches!(mode, Mode::Code | Mode::LineComment) {
            let width = js_line_terminator_width(bytes, index);
            if width != 0 {
                if matches!(mode, Mode::LineComment) {
                    mode = Mode::Code;
                }
                index += width;
                continue;
            }
        }
        if matches!(mode, Mode::Code) {
            let character = source[index..].chars().next().unwrap();
            if js_whitespace(character) {
                index += character.len_utf8();
                continue;
            }
        }
        match mode {
            Mode::Code => {
                let condition_start = pending_condition;
                let property_name = member_dot;
                pending_condition = false;
                member_dot = false;
                match (current, next) {
                    (b'/', Some(b'/')) => {
                        mode = Mode::LineComment;
                        pending_condition = condition_start;
                        member_dot = property_name;
                        index += 1;
                    }
                    (b'/', Some(b'*')) => {
                        mode = Mode::BlockComment;
                        pending_condition = condition_start;
                        member_dot = property_name;
                        index += 1;
                    }
                    (b'/', _) if expect_operand => {
                        mode = Mode::Regex;
                    }
                    (b'\'', _) => {
                        mode = Mode::Single;
                        can_start_object = false;
                    }
                    (b'"', _) => {
                        mode = Mode::Double;
                        can_start_object = false;
                    }
                    (b'`', _) => {
                        mode = Mode::Template;
                        can_start_object = false;
                    }
                    (b'(' | b'[' | b'{', _) => {
                        if !expect_operand && matches!(current, b'(' | b'[') && !condition_start {
                            count_recursive_step(&mut recursive_chain)?;
                        }
                        let kind = match current {
                            b'(' if condition_start => b'c',
                            b'(' if !expect_operand => b'a',
                            b'[' if !expect_operand => b'i',
                            b'{' if can_start_object => b'o',
                            _ => current,
                        };
                        delimiters.push(Delimiter {
                            kind,
                            outer_chain: recursive_chain,
                        });
                        if delimiters.len() > MAX_SOURCE_NESTING {
                            bail!(
                                "script entry exceeds the {MAX_SOURCE_NESTING} level syntax nesting limit"
                            );
                        }
                        expect_operand = true;
                        can_start_object = matches!(current, b'(' | b'[');
                    }
                    (b')' | b']' | b'}', _) => {
                        let Some(delimiter) = delimiters.pop() else {
                            bail!("script entry has invalid delimiter nesting");
                        };
                        let open = delimiter.kind;
                        if open == b'$' && current == b'}' {
                            mode = Mode::Template;
                        } else if !matches!(
                            (open, current),
                            (b'(', b')')
                                | (b'a', b')')
                                | (b'c', b')')
                                | (b'[', b']')
                                | (b'i', b']')
                                | (b'{', b'}')
                                | (b'o', b'}')
                        ) {
                            bail!("script entry has invalid delimiter nesting");
                        }
                        recursive_chain = delimiter.outer_chain;
                        expect_operand = open == b'c';
                        can_start_object = false;
                    }
                    (b';', _) => {
                        recursive_chain = 0;
                        expect_operand = true;
                        can_start_object = false;
                    }
                    (b' ' | b'\t' | b'\x0b' | b'\x0c', _) => {
                        pending_condition = condition_start;
                        member_dot = property_name;
                    }
                    (c, _) if c.is_ascii_alphabetic() || c == b'_' || c == b'$' || c >= 0x80 => {
                        let start = index;
                        let end = source_token_end(source, start, false);
                        index = end - 1;
                        let word = &source[start..end];
                        if !property_name {
                            if matches!(word, "const" | "let" | "var" | "export") {
                                recursive_chain = 0;
                            } else if matches!(
                                word,
                                "delete"
                                    | "void"
                                    | "typeof"
                                    | "new"
                                    | "await"
                                    | "yield"
                                    | "in"
                                    | "instanceof"
                                    | "of"
                                    | "if"
                                    | "while"
                                    | "for"
                                    | "with"
                                    | "switch"
                                    | "catch"
                                    | "else"
                                    | "do"
                            ) {
                                count_recursive_step(&mut recursive_chain)?;
                            }
                        }
                        expect_operand = !property_name
                            && matches!(
                                word,
                                "return"
                                    | "throw"
                                    | "case"
                                    | "delete"
                                    | "void"
                                    | "typeof"
                                    | "new"
                                    | "await"
                                    | "yield"
                                    | "default"
                                    | "in"
                                    | "of"
                                    | "instanceof"
                            );
                        pending_condition = !property_name
                            && matches!(word, "if" | "while" | "for" | "with" | "switch" | "catch");
                        can_start_object = !property_name
                            && matches!(
                                word,
                                "return" | "throw" | "case" | "default" | "yield" | "await"
                            );
                    }
                    (c, _) if c.is_ascii_digit() => {
                        index = source_token_end(source, index, true) - 1;
                        expect_operand = false;
                        can_start_object = false;
                    }
                    (b'+' | b'-', Some(other)) if current == other => {
                        count_recursive_step(&mut recursive_chain)?;
                        index += 1;
                    }
                    (b',', _) => {
                        if delimiters
                            .last()
                            .is_some_and(|delimiter| matches!(delimiter.kind, b'[' | b'o' | b'a'))
                        {
                            recursive_chain = delimiters.last().unwrap().outer_chain;
                        } else {
                            count_recursive_step(&mut recursive_chain)?;
                        }
                        expect_operand = true;
                        can_start_object = true;
                    }
                    _ => {
                        if matches!(
                            current,
                            b'/' | b'='
                                | b'+'
                                | b'-'
                                | b'*'
                                | b'!'
                                | b'~'
                                | b'%'
                                | b'^'
                                | b'&'
                                | b'|'
                                | b'<'
                                | b'>'
                                | b'?'
                                | b'.'
                        ) {
                            count_recursive_step(&mut recursive_chain)?;
                        }
                        member_dot = current == b'.';
                        expect_operand = matches!(
                            current,
                            b'/' | b'='
                                | b'+'
                                | b'-'
                                | b'*'
                                | b'!'
                                | b'~'
                                | b'%'
                                | b'^'
                                | b'&'
                                | b'|'
                                | b'<'
                                | b'>'
                                | b'?'
                                | b':'
                        );
                        can_start_object = matches!(
                            current,
                            b'=' | b'+' | b'-' | b'*' | b'/' | b'?' | b':' | b'&' | b'|'
                        );
                    }
                }
            }
            Mode::Single | Mode::Double | Mode::Template => {
                if current == b'\\' {
                    index += 1;
                } else if matches!(mode, Mode::Single) && current == b'\''
                    || matches!(mode, Mode::Double) && current == b'"'
                    || matches!(mode, Mode::Template) && current == b'`'
                {
                    mode = Mode::Code;
                } else if matches!(mode, Mode::Template) && current == b'$' && next == Some(b'{') {
                    delimiters.push(Delimiter {
                        kind: b'$',
                        outer_chain: recursive_chain,
                    });
                    if delimiters.len() > MAX_SOURCE_NESTING {
                        bail!(
                            "script entry exceeds the {MAX_SOURCE_NESTING} level syntax nesting limit"
                        );
                    }
                    mode = Mode::Code;
                    expect_operand = true;
                    can_start_object = true;
                    index += 1;
                }
            }
            Mode::Regex => {
                if current == b'\\' {
                    index += 1;
                } else if current == b'[' {
                    mode = Mode::RegexClass;
                } else if current == b'/' {
                    mode = Mode::Code;
                    expect_operand = false;
                    can_start_object = false;
                    while index + 1 < bytes.len() && bytes[index + 1].is_ascii_alphabetic() {
                        index += 1;
                    }
                }
            }
            Mode::RegexClass => {
                if current == b'\\' {
                    index += 1;
                } else if current == b']' {
                    mode = Mode::Regex;
                }
            }
            Mode::LineComment => {}
            Mode::BlockComment => {
                if current == b'*' && next == Some(b'/') {
                    mode = Mode::Code;
                    index += 1;
                }
            }
        }
        index += 1;
    }
    Ok(())
}

fn count_recursive_step(count: &mut usize) -> Result<()> {
    *count += 1;
    if *count > MAX_SOURCE_RECURSIVE_STEPS {
        bail!(
            "script entry exceeds the {MAX_SOURCE_RECURSIVE_STEPS} step recursive syntax complexity limit"
        );
    }
    Ok(())
}

fn source_token_end(source: &str, start: usize, allow_dot: bool) -> usize {
    let mut end = start;
    for (offset, character) in source[start..].char_indices() {
        if offset != 0
            && !character.is_alphanumeric()
            && character != '_'
            && character != '$'
            && !(allow_dot && character == '.')
            && (character.is_ascii() || js_whitespace(character))
        {
            break;
        }
        end = start + offset + character.len_utf8();
    }
    end
}

fn js_whitespace(character: char) -> bool {
    character.is_whitespace() || character == '\u{feff}'
}

fn js_line_terminator_width(bytes: &[u8], index: usize) -> usize {
    match bytes[index] {
        b'\r' | b'\n' => 1,
        0xe2 if bytes[index..].starts_with(&[0xe2, 0x80, 0xa8])
            || bytes[index..].starts_with(&[0xe2, 0x80, 0xa9]) =>
        {
            3
        }
        _ => 0,
    }
}

fn read_bounded_metadata(path: &Path, description: &str) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .with_context(|| format!("could not read {description} {}", path.display()))?
        .take(MAX_METADATA_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("could not read {description} {}", path.display()))?;
    if bytes.len() as u64 > MAX_METADATA_BYTES {
        bail!(
            "{description} {} exceeds the 65536 byte metadata limit",
            path.display()
        );
    }
    Ok(bytes)
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn validate_manifest(manifest: &ScriptPackageManifest) -> Result<()> {
    if manifest.format != MANIFEST_FORMAT {
        bail!("unrecognized script package format `{}`", manifest.format);
    }
    if manifest.api_version != API_VERSION {
        bail!(
            "unsupported script API version {}; expected {API_VERSION}",
            manifest.api_version
        );
    }
    validate_entry_name(&manifest.entry)?;

    for permission in &manifest.permissions {
        if !ALLOWED_PERMISSIONS.contains(&permission.as_str()) {
            bail!("unknown script permission `{permission}`");
        }
    }
    if manifest
        .permissions
        .windows(2)
        .any(|pair| pair[0] >= pair[1])
    {
        bail!("script permissions must be sorted and unique");
    }
    Ok(())
}

fn validate_entry_name(value: &str) -> Result<()> {
    if value.is_empty()
        || value.starts_with('/')
        || value.contains('\\')
        || value.contains(':')
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        bail!("unsafe script entry path `{value}`; use a normalized package-relative path");
    }

    let path = Path::new(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("unsafe script entry path `{value}`; use a normalized package-relative path");
    }

    match path.extension().and_then(|extension| extension.to_str()) {
        Some("js" | "mjs") => Ok(()),
        _ => bail!("script entry `{value}` must have a .js or .mjs extension"),
    }
}

fn resolve_regular_entry(root: &Path, relative: &str) -> Result<PathBuf> {
    validate_entry_name(relative)?;
    let mut current = root.to_path_buf();
    let components = Path::new(relative).components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current)
            .with_context(|| format!("script entry `{relative}` does not exist"))?;
        if metadata.file_type().is_symlink() {
            bail!("script entry `{relative}` must not contain symbolic links");
        }
        let final_component = index + 1 == components.len();
        if final_component && !metadata.is_file() {
            bail!("script entry `{relative}` is not a regular file");
        }
        if !final_component && !metadata.is_dir() {
            bail!("script entry parent in `{relative}` is not a directory");
        }
    }
    let canonical = fs::canonicalize(&current)
        .with_context(|| format!("could not resolve script entry `{relative}`"))?;
    if !canonical.starts_with(root) {
        bail!("script entry `{relative}` resolves outside its package root");
    }
    Ok(canonical)
}

fn validate_output_directory(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!(
                "refusing to use a symbolic link as script output: {}",
                path.display()
            );
        }
        Ok(metadata) if metadata.is_dir() => Ok(true),
        Ok(_) => bail!(
            "script output exists and is not a directory: {}",
            path.display()
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("could not inspect {}", path.display())),
    }
}

fn validate_previous_pack(output: &Path) -> Result<()> {
    let (files, directories) = collect_output_inventory(output)?;
    if files.is_empty() && directories.is_empty() {
        return Ok(());
    }

    let marker_path = output.join(OWNERSHIP_NAME);
    let marker_metadata = fs::symlink_metadata(&marker_path)
        .context("refusing to replace script output without a CLI ownership marker")?;
    if !marker_metadata.is_file() || marker_metadata.file_type().is_symlink() {
        bail!("existing script output ownership marker is not a regular file");
    }
    let marker: PackOwnership = serde_json::from_slice(&read_bounded_metadata(
        &marker_path,
        "script ownership marker",
    )?)
    .context("existing script output ownership marker is invalid")?;
    if marker.format != "bornengine-cli-script-pack-v1" {
        bail!("existing script output ownership marker has an unknown format");
    }

    let manifest_path = output.join(MANIFEST_NAME);
    match fs::symlink_metadata(&manifest_path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
        Ok(_) => bail!("existing script output manifest is not a regular file"),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            bail!("refusing to replace script output containing files not owned by BornEngine")
        }
        Err(error) => {
            return Err(error).context("could not inspect existing script output manifest");
        }
    };
    let bytes = read_bounded_metadata(&manifest_path, "existing script output manifest")?;
    if marker.manifest_sha256 != sha256(&bytes) {
        bail!("refusing to replace modified script output manifest");
    }
    let previous: ScriptPackageManifest = serde_json::from_slice(&bytes)
        .context("existing script output has an invalid BornEngine manifest")?;
    validate_manifest(&previous).context("existing script output manifest is invalid")?;
    let previous_entry = resolve_regular_entry(output, &previous.entry)
        .context("existing script output entry is invalid")?;
    let previous_bytes = read_bounded_entry(&previous_entry, &previous.entry)?;
    if marker.entry_sha256 != sha256(&previous_bytes) {
        bail!("refusing to replace modified script output entry");
    }

    let expected_files = BTreeSet::from([
        OWNERSHIP_NAME.to_owned(),
        MANIFEST_NAME.to_owned(),
        previous.entry.clone(),
    ]);
    let mut expected_directories = BTreeSet::new();
    let mut parent = Path::new(&previous.entry).parent();
    while let Some(directory) = parent {
        if directory.as_os_str().is_empty() {
            break;
        }
        expected_directories.insert(path_string(directory));
        parent = directory.parent();
    }
    if files != expected_files || directories != expected_directories {
        bail!("refusing to replace script output with untracked or missing content");
    }
    Ok(())
}

fn collect_output_inventory(root: &Path) -> Result<(BTreeSet<String>, BTreeSet<String>)> {
    fn visit(
        root: &Path,
        directory: &Path,
        files: &mut BTreeSet<String>,
        directories: &mut BTreeSet<String>,
    ) -> Result<()> {
        let mut entries = fs::read_dir(directory)
            .with_context(|| format!("could not inspect script output {}", directory.display()))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        entries.sort();
        for path in entries {
            let metadata = fs::symlink_metadata(&path)
                .with_context(|| format!("could not inspect script output {}", path.display()))?;
            if metadata.file_type().is_symlink() {
                bail!("refusing to replace script output containing symbolic links");
            }
            let relative = path
                .strip_prefix(root)
                .context("script output inventory escaped its root")?;
            let name = path_string(relative);
            if metadata.is_dir() {
                directories.insert(name);
                visit(root, &path, files, directories)?;
            } else if metadata.is_file() {
                files.insert(name);
            } else {
                bail!("refusing to replace script output containing a non-regular file");
            }
        }
        Ok(())
    }

    let mut files = BTreeSet::new();
    let mut directories = BTreeSet::new();
    visit(root, root, &mut files, &mut directories)?;
    Ok((files, directories))
}

fn publish_pack(
    output: &Path,
    stage: &mut ScriptPackStage,
    staged_files: &Path,
    output_exists: bool,
) -> Result<()> {
    let backup = stage.path.join("previous");
    if output_exists {
        fs::rename(output, &backup).with_context(|| {
            format!(
                "could not stage existing script output {}",
                output.display()
            )
        })?;
    }
    if let Err(publish_error) = fs::rename(staged_files, output) {
        if output_exists {
            if let Err(restore_error) = fs::rename(&backup, output) {
                stage.preserve();
                bail!(
                    "script pack failed: {publish_error}; previous output is preserved at {} because rollback failed: {restore_error}",
                    backup.display()
                );
            }
        }
        return Err(publish_error)
            .with_context(|| format!("could not publish script output {}", output.display()));
    }
    Ok(())
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

struct ScriptPackStage {
    path: PathBuf,
    preserve_on_drop: bool,
}

impl ScriptPackStage {
    fn create(parent: &Path) -> Result<Self> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        for attempt in 0_u32..u32::MAX {
            let candidate = parent.join(format!(
                ".bornengine-script-pack-{}-{timestamp}-{attempt}",
                std::process::id()
            ));
            match fs::create_dir(&candidate) {
                Ok(()) => {
                    return Ok(Self {
                        path: candidate,
                        preserve_on_drop: false,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "could not create script staging directory {}",
                            candidate.display()
                        )
                    });
                }
            }
        }
        bail!("could not allocate a unique script staging directory")
    }

    fn preserve(&mut self) {
        // On a failed rollback, keep the previous package where it can be recovered.
        self.preserve_on_drop = true;
    }
}

impl Drop for ScriptPackStage {
    fn drop(&mut self) {
        if !self.preserve_on_drop {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
