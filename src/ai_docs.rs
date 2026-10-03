use anyhow::{Context, Result, bail};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const GUIDE: &str = include_str!("../templates/ai_docs.md");

pub fn add_to_file(name: &Path) -> Result<PathBuf> {
    let destination = markdown_path(name)?;
    write_new_file(&destination)?;
    Ok(destination)
}

fn markdown_path(name: &Path) -> Result<PathBuf> {
    if name.file_name().is_none() {
        bail!("AI guide filename must include a file name");
    }

    let text = name.as_os_str().to_string_lossy();
    if text.to_ascii_lowercase().ends_with(".md") {
        return Ok(name.to_path_buf());
    }

    let mut output = name.as_os_str().to_os_string();
    output.push(".md");
    Ok(PathBuf::from(output))
}

fn write_new_file(destination: &Path) -> Result<()> {
    let mut file = match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            bail!(
                "refusing to overwrite existing file: {}",
                destination.display()
            );
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("could not create {}", destination.display()));
        }
    };
    if let Err(error) = file.write_all(GUIDE.as_bytes()) {
        drop(file);
        let _ = fs::remove_file(destination);
        return Err(error).with_context(|| format!("could not write {}", destination.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{GUIDE, add_to_file};
    use std::fs;

    #[test]
    fn appends_markdown_extension_when_missing() {
        let directory = tempfile::tempdir().unwrap();
        let path = add_to_file(&directory.path().join("assistant-guide")).unwrap();

        assert_eq!(path.file_name().unwrap(), "assistant-guide.md");
        assert_eq!(fs::read_to_string(path).unwrap(), GUIDE);
    }

    #[test]
    fn preserves_a_markdown_extension_without_adding_a_second_one() {
        let directory = tempfile::tempdir().unwrap();
        let path = add_to_file(&directory.path().join("assistant-guide.md")).unwrap();

        assert_eq!(path.file_name().unwrap(), "assistant-guide.md");
        assert_eq!(fs::read_to_string(path).unwrap(), GUIDE);
    }

    #[test]
    fn refuses_to_overwrite_an_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let existing = directory.path().join("assistant-guide.md");
        fs::write(&existing, "keep this file").unwrap();

        let error = add_to_file(&directory.path().join("assistant-guide")).unwrap_err();

        assert!(error.to_string().contains("refusing to overwrite"));
        assert_eq!(fs::read_to_string(existing).unwrap(), "keep this file");
    }

    #[test]
    fn rejects_an_empty_filename() {
        let error = add_to_file(std::path::Path::new("")).unwrap_err();

        assert!(error.to_string().contains("must include a file name"));
    }
}
