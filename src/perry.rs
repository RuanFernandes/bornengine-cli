use anyhow::{Context, Result, bail};
use directories::BaseDirs;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

const RELEASES_API: &str = "https://api.github.com/repos/RuanFernandes/BornEngine/releases";
const RELEASES_API_ENV: &str = "BORNENGINE_RELEASES_API";
const CHECKSUMS_ASSET: &str = "perry-SHA256SUMS.txt";
const PROGRAM_ENV: &str = "BORNENGINE_PERRY";
const CURRENT_FILE: &str = "current";
const COMMIT_FILE: &str = "PERRY_COMMIT";

pub struct Installed {
    pub release: String,
    pub program: PathBuf,
    pub commit: Option<String>,
}

pub fn program() -> String {
    let override_path = std::env::var(PROGRAM_ENV).ok();
    let installed = installed_program().ok().flatten();
    resolve_program(override_path.as_deref(), installed.as_deref())
}

pub fn install(release: Option<&str>) -> Result<Installed> {
    let asset = asset_for(std::env::consts::OS, std::env::consts::ARCH)?;
    let archive_name = archive_name(asset);
    let agent = ureq::Agent::new_with_defaults();

    let release_json = get_json(&agent, &release_url(release)?)?;
    let tag = release_json["tag_name"]
        .as_str()
        .context("the BornEngine release has no tag_name")?
        .to_owned();
    validate_tag(&tag)?;

    let sums = get_text(&agent, &asset_url(&release_json, CHECKSUMS_ASSET)?)?;
    let expected = expected_checksum(&sums, &archive_name)
        .with_context(|| format!("{CHECKSUMS_ASSET} has no entry for {archive_name}"))?;

    let root = managed_root()?;
    let staging = root.join(format!(".{tag}.partial"));
    remove_dir_if_exists(&staging)?;
    fs::create_dir_all(&staging)?;
    let archive_path = staging.join(&archive_name);
    let actual = download(
        &agent,
        &asset_url(&release_json, &archive_name)?,
        &archive_path,
    )?;
    if !actual.eq_ignore_ascii_case(&expected) {
        remove_dir_if_exists(&staging)?;
        bail!("checksum mismatch for {archive_name}: expected {expected}, got {actual}");
    }

    extract(&archive_path, &staging)?;
    let extracted = staging.join("perry");
    if !extracted.join(executable_name()).is_file() {
        remove_dir_if_exists(&staging)?;
        bail!(
            "{archive_name} does not contain perry/{}",
            executable_name()
        );
    }
    let install_dir = root.join(&tag);
    remove_dir_if_exists(&install_dir)?;
    fs::create_dir_all(&install_dir)?;
    fs::rename(&extracted, install_dir.join("perry"))?;
    remove_dir_if_exists(&staging)?;
    fs::write(root.join(CURRENT_FILE), format!("{tag}\n"))?;

    let perry_dir = install_dir.join("perry");
    let commit = fs::read_to_string(perry_dir.join(COMMIT_FILE))
        .ok()
        .map(|text| text.trim().to_owned());
    Ok(Installed {
        release: tag,
        program: perry_dir.join(executable_name()),
        commit,
    })
}

fn resolve_program(override_path: Option<&str>, installed: Option<&Path>) -> String {
    if let Some(path) = override_path.filter(|path| !path.is_empty()) {
        return path.to_owned();
    }
    if let Some(path) = installed {
        return path.to_string_lossy().into_owned();
    }
    "perry".to_owned()
}

fn installed_program() -> Result<Option<PathBuf>> {
    let root = managed_root()?;
    let current = match fs::read_to_string(root.join(CURRENT_FILE)) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let tag = current.trim();
    if validate_tag(tag).is_err() {
        return Ok(None);
    }
    let program = root.join(tag).join("perry").join(executable_name());
    Ok(program.is_file().then_some(program))
}

fn managed_root() -> Result<PathBuf> {
    let base_dirs = BaseDirs::new().context("could not determine the user data directory")?;
    Ok(base_dirs.data_dir().join("bornengine").join("perry"))
}

fn asset_for(os: &str, arch: &str) -> Result<&'static str> {
    match (os, arch) {
        ("linux", "x86_64") => Ok("linux-x86_64"),
        ("windows", "x86_64") => Ok("windows-x86_64"),
        ("macos", "x86_64") => Ok("macos-x86_64"),
        ("macos", "aarch64") => Ok("macos-aarch64"),
        _ => bail!("no Perry archive is published for {os}-{arch}"),
    }
}

fn archive_name(asset: &str) -> String {
    if asset.starts_with("windows-") {
        format!("perry-{asset}.zip")
    } else {
        format!("perry-{asset}.tar.gz")
    }
}

fn executable_name() -> &'static str {
    if cfg!(windows) { "perry.exe" } else { "perry" }
}

fn release_url(release: Option<&str>) -> Result<String> {
    let api = std::env::var(RELEASES_API_ENV).unwrap_or_else(|_| RELEASES_API.to_owned());
    match release {
        None => Ok(format!("{api}/latest")),
        Some(tag) => {
            validate_tag(tag)?;
            Ok(format!("{api}/tags/{tag}"))
        }
    }
}

fn validate_tag(tag: &str) -> Result<()> {
    let valid = !tag.is_empty()
        && !tag.starts_with('.')
        && tag.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_')
        });
    if valid {
        Ok(())
    } else {
        bail!("invalid release tag {tag:?}")
    }
}

fn expected_checksum(sums: &str, file_name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let hash = fields.next()?;
        let name = fields.next()?.trim_start_matches('*');
        (name == file_name).then(|| hash.to_ascii_lowercase())
    })
}

fn asset_url(release: &Value, name: &str) -> Result<String> {
    release["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|asset| asset["name"].as_str() == Some(name))
        .and_then(|asset| asset["browser_download_url"].as_str())
        .map(str::to_owned)
        .with_context(|| format!("the BornEngine release has no asset named {name}"))
}

fn get_json(agent: &ureq::Agent, url: &str) -> Result<Value> {
    let response = github_get(agent, url)?;
    response
        .into_body()
        .read_json::<Value>()
        .with_context(|| format!("could not parse {url}"))
}

fn get_text(agent: &ureq::Agent, url: &str) -> Result<String> {
    let response = github_get(agent, url)?;
    response
        .into_body()
        .read_to_string()
        .with_context(|| format!("could not read {url}"))
}

fn github_get(agent: &ureq::Agent, url: &str) -> Result<ureq::http::Response<ureq::Body>> {
    agent
        .get(url)
        .header("User-Agent", "bornengine-cli")
        .header("Accept", "application/vnd.github+json")
        .call()
        .with_context(|| format!("could not GET {url}"))
}

fn download(agent: &ureq::Agent, url: &str, destination: &Path) -> Result<String> {
    let response = github_get(agent, url)?;
    let mut reader = response.into_body().into_reader();
    let mut file = File::create(destination)
        .with_context(|| format!("could not create {}", destination.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1 << 16];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        file.write_all(&buffer[..read])?;
    }
    file.sync_all()?;
    Ok(format!("{:x}", hasher.finalize()))
}

fn extract(archive: &Path, destination: &Path) -> Result<()> {
    let is_zip = archive.extension().and_then(|extension| extension.to_str()) == Some("zip");
    let status = Command::new(tar_program())
        .arg(if is_zip { "-xf" } else { "-xzf" })
        .arg(archive)
        .arg("-C")
        .arg(destination)
        .status()
        .context("could not run tar to extract the Perry archive")?;
    if !status.success() {
        bail!("tar failed to extract {} ({status})", archive.display());
    }
    Ok(())
}

fn tar_program() -> PathBuf {
    if cfg!(windows) {
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            return PathBuf::from(system_root).join("System32").join("tar.exe");
        }
    }
    PathBuf::from("tar")
}

fn remove_dir_if_exists(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path).with_context(|| format!("could not remove {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_supported_hosts_to_release_assets() {
        assert_eq!(asset_for("linux", "x86_64").unwrap(), "linux-x86_64");
        assert_eq!(asset_for("windows", "x86_64").unwrap(), "windows-x86_64");
        assert_eq!(asset_for("macos", "x86_64").unwrap(), "macos-x86_64");
        assert_eq!(asset_for("macos", "aarch64").unwrap(), "macos-aarch64");
        assert!(asset_for("linux", "aarch64").is_err());
    }

    #[test]
    fn windows_archives_are_zip_and_others_tar_gz() {
        assert_eq!(archive_name("windows-x86_64"), "perry-windows-x86_64.zip");
        assert_eq!(archive_name("linux-x86_64"), "perry-linux-x86_64.tar.gz");
    }

    #[test]
    fn finds_checksum_for_the_named_archive() {
        let sums = "AAAA11  perry-linux-x86_64.tar.gz\nBBBB22 *perry-macos-aarch64.tar.gz\n";
        assert_eq!(
            expected_checksum(sums, "perry-linux-x86_64.tar.gz").as_deref(),
            Some("aaaa11")
        );
        assert_eq!(
            expected_checksum(sums, "perry-macos-aarch64.tar.gz").as_deref(),
            Some("bbbb22")
        );
        assert_eq!(expected_checksum(sums, "perry-windows-x86_64.zip"), None);
    }

    #[test]
    fn accepts_plain_release_tags_only() {
        assert!(validate_tag("v0.16.0").is_ok());
        assert!(validate_tag("v0.16.0-rc_1").is_ok());
        assert!(validate_tag("").is_err());
        assert!(validate_tag(".hidden").is_err());
        assert!(validate_tag("../escape").is_err());
        assert!(validate_tag("a/b").is_err());
    }

    #[test]
    fn explicit_override_beats_installed_perry_beats_path() {
        let installed = Path::new("/data/bornengine/perry/v1/perry/perry");
        assert_eq!(
            resolve_program(Some("/opt/perry"), Some(installed)),
            "/opt/perry"
        );
        assert_eq!(
            resolve_program(Some(""), Some(installed)),
            installed.to_string_lossy().into_owned()
        );
        assert_eq!(
            resolve_program(None, Some(installed)),
            installed.to_string_lossy().into_owned()
        );
        assert_eq!(resolve_program(None, None), "perry");
    }

    #[test]
    fn resolves_asset_urls_from_release_json() {
        let release = json!({
            "assets": [
                { "name": "perry-linux-x86_64.tar.gz", "browser_download_url": "https://example.test/linux" }
            ]
        });
        assert_eq!(
            asset_url(&release, "perry-linux-x86_64.tar.gz").unwrap(),
            "https://example.test/linux"
        );
        assert!(asset_url(&release, "perry-macos-aarch64.tar.gz").is_err());
    }
}
