use crate::engine_package::{
    ENGINE_PACKAGES, engine_subpath, is_engine_package, native_library_allow_pattern,
};
use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

const GENERATED_FILES: &[&str] = &["package.json", "main.ts", ".gitignore", "README.md"];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectSpec {
    pub engine_package: String,
    pub engine_spec: String,
}

pub fn create_project(parent: &Path, project_name: &str, spec: &ProjectSpec) -> Result<PathBuf> {
    validate_project_name(project_name)?;
    validate_spec(spec)?;
    if !parent.is_dir() {
        bail!(
            "project parent directory does not exist: {}",
            parent.display()
        );
    }

    let root = parent.join(project_name);
    let root_was_created = match fs::symlink_metadata(&root) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!(
                "project path must not be a symbolic link: {}",
                root.display()
            );
        }
        Ok(metadata) if !metadata.is_dir() => {
            bail!(
                "project path already exists and is not a directory: {}",
                root.display()
            );
        }
        Ok(_) => {
            if fs::read_dir(&root)
                .with_context(|| format!("could not inspect {}", root.display()))?
                .next()
                .is_some()
            {
                bail!(
                    "project directory already contains files: {}",
                    root.display()
                );
            }
            false
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&root).with_context(|| {
                format!("could not create project directory {}", root.display())
            })?;
            true
        }
        Err(error) => {
            return Err(error).with_context(|| format!("could not inspect {}", root.display()));
        }
    };

    if let Err(error) = write_project_files(&root, project_name, spec) {
        if root_was_created {
            let _ = fs::remove_dir(&root);
        }
        return Err(error);
    }
    Ok(root)
}

pub fn initialize_project(root: &Path, project_name: &str, spec: &ProjectSpec) -> Result<PathBuf> {
    validate_spec(spec)?;
    let metadata = fs::symlink_metadata(root)
        .with_context(|| format!("project directory does not exist: {}", root.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("project path must be a real directory: {}", root.display());
    }
    write_project_files(root, project_name, spec)?;
    Ok(root.to_path_buf())
}

fn write_project_files(root: &Path, project_name: &str, spec: &ProjectSpec) -> Result<()> {
    validate_project_name(project_name)?;
    validate_spec(spec)?;
    for name in GENERATED_FILES {
        let path = root.join(name);
        if fs::symlink_metadata(&path).is_ok() {
            bail!("project file already exists: {}", path.display());
        }
    }

    let package_name = npm_project_name(project_name)?;
    let files = generated_files(project_name, &package_name, spec)?;
    let mut created = Vec::new();
    for (name, contents) in files {
        let path = root.join(name);
        let result = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .with_context(|| format!("could not create {}", path.display()))
            .and_then(|mut file| {
                created.push(path.clone());
                file.write_all(contents.as_bytes())
                    .with_context(|| format!("could not write {}", path.display()))
            });
        if let Err(error) = result {
            for created_path in created.iter().rev() {
                let _ = fs::remove_file(created_path);
            }
            return Err(error);
        }
    }
    Ok(())
}

fn generated_files(
    project_title: &str,
    package_name: &str,
    spec: &ProjectSpec,
) -> Result<Vec<(&'static str, String)>> {
    let mut package = Map::new();
    package.insert("name".to_owned(), json!(package_name));
    package.insert("private".to_owned(), json!(true));
    package.insert("version".to_owned(), json!("0.1.0"));
    package.insert(
        "dependencies".to_owned(),
        json!({ spec.engine_package.clone(): spec.engine_spec.clone() }),
    );
    package.insert(
        "perry".to_owned(),
        json!({
            "allow": {
                "nativeLibrary": [native_library_allow_pattern(&spec.engine_package)]
            }
        }),
    );
    let package_json = serde_json::to_string_pretty(&Value::Object(package))? + "\n";
    let core = engine_subpath(&spec.engine_package, "core");
    let shapes = engine_subpath(&spec.engine_package, "shapes");
    let main_ts = format!(
        r#"import {{ initWindow, runGame, clearBackground, setTargetFPS, setDirect2DMode }} from "{core}";
import {{ drawRect }} from "{shapes}";

initWindow(800, 450, "BornEngine Game");
setTargetFPS(60);
setDirect2DMode(true);

runGame(() => {{
  clearBackground({{ r: 22, g: 26, b: 36, a: 255 }});
  drawRect(300, 160, 200, 130, {{ r: 72, g: 156, b: 220, a: 255 }});
}});
"#
    );
    let gitignore = "node_modules/\n.perry-cache/\n.perry-dev/\n.bornengine/\ntarget/\ndist/\n";
    let readme = format!(
        "# {project_title}\n\nA small game made with BornEngine and Perry.\n\n## Run\n\n```sh\nbornengine run main.ts\n```\n\n## Build\n\n```sh\nbornengine build main.ts --os linux\nbornengine build main.ts --os windows\n```\n"
    );
    Ok(vec![
        ("package.json", package_json),
        ("main.ts", main_ts),
        (".gitignore", gitignore.to_owned()),
        ("README.md", readme),
    ])
}

fn validate_spec(spec: &ProjectSpec) -> Result<()> {
    if !is_engine_package(&spec.engine_package) {
        bail!(
            "unsupported BornEngine package name `{}`",
            spec.engine_package
        );
    }
    if spec.engine_spec.trim().is_empty() {
        bail!("engine dependency specification cannot be empty");
    }
    Ok(())
}

pub fn validate_project_name(name: &str) -> Result<()> {
    let path = Path::new(name);
    let mut components = path.components();
    let single_name =
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();
    let invalid_character = name.chars().any(|character| {
        character.is_control()
            || matches!(
                character,
                '<' | '>' | ':' | '"' | '|' | '?' | '*' | '/' | '\\'
            )
    });
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let reserved = ["con", "prn", "aux", "nul"].contains(&stem.as_str())
        || ["com", "lpt"].into_iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|suffix| {
                suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9')
            })
        });
    if !single_name
        || name.is_empty()
        || name == "."
        || name == ".."
        || name.ends_with([' ', '.'])
        || invalid_character
        || reserved
    {
        bail!(
            "project name must be a single directory name without traversal or platform-reserved characters"
        );
    }
    Ok(())
}

fn npm_project_name(name: &str) -> Result<String> {
    let mut normalized = String::new();
    let mut previous_was_lower_or_digit = false;
    for character in name.chars() {
        if character.is_ascii_uppercase() {
            if previous_was_lower_or_digit && !normalized.ends_with('-') {
                normalized.push('-');
            }
            normalized.push(character.to_ascii_lowercase());
            previous_was_lower_or_digit = true;
        } else if character.is_ascii_lowercase() || character.is_ascii_digit() {
            normalized.push(character);
            previous_was_lower_or_digit = true;
        } else if character == '.' || character == '_' || character == '-' {
            if !normalized.is_empty() && !normalized.ends_with('-') {
                normalized.push(if character == '-' { '-' } else { character });
            }
            previous_was_lower_or_digit = false;
        } else if !normalized.is_empty() && !normalized.ends_with('-') {
            normalized.push('-');
            previous_was_lower_or_digit = false;
        }
    }
    while normalized.ends_with(['-', '.']) {
        normalized.pop();
    }
    if normalized.is_empty() || normalized.len() > 214 || normalized.starts_with('.') {
        bail!("project name cannot be converted to a valid npm package name");
    }
    Ok(normalized)
}

pub fn read_package_json(root: &Path) -> Result<Value> {
    let path = root.join("package.json");
    let contents = fs::read_to_string(&path)
        .with_context(|| format!("could not read project metadata at {}", path.display()))?;
    serde_json::from_str(&contents).with_context(|| format!("invalid JSON in {}", path.display()))
}

pub fn write_package_json(root: &Path, package: &Value) -> Result<()> {
    let path = root.join("package.json");
    let metadata = fs::symlink_metadata(&path)
        .with_context(|| format!("could not inspect {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!(
            "refusing to update non-regular project metadata {}",
            path.display()
        );
    }
    let mut contents = serde_json::to_string_pretty(package)?;
    contents.push('\n');
    fs::write(&path, contents).with_context(|| format!("could not write {}", path.display()))
}

pub fn project_name(root: &Path) -> Result<Option<String>> {
    Ok(read_package_json(root)?
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_owned))
}

pub fn is_bornengine_project(root: &Path) -> Result<bool> {
    let package = read_package_json(root)?;
    let dependency_match = ["dependencies", "devDependencies", "optionalDependencies"]
        .into_iter()
        .filter_map(|section| package.get(section).and_then(Value::as_object))
        .flat_map(|dependencies| dependencies.keys())
        .any(|name| is_engine_package(name));
    let native_module_match = package
        .pointer("/perry/nativeLibrary/module")
        .and_then(Value::as_str)
        .is_some_and(is_engine_package);
    let allowlist_match = package
        .pointer("/perry/allow/nativeLibrary")
        .and_then(Value::as_array)
        .is_some_and(|entries| {
            entries.iter().filter_map(Value::as_str).any(|entry| {
                ENGINE_PACKAGES
                    .iter()
                    .any(|package_name| entry == native_library_allow_pattern(package_name))
            })
        });
    Ok(dependency_match || native_module_match || allowlist_match)
}

pub fn find_project_root(start: &Path) -> Result<Option<PathBuf>> {
    let mut current = if start.is_file() {
        start
            .parent()
            .context("project start path has no parent directory")?
            .to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        if current.join("package.json").is_file() && is_bornengine_project(&current)? {
            return Ok(Some(current));
        }
        if !current.pop() {
            return Ok(None);
        }
    }
}
