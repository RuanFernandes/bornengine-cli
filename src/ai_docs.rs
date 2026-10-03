use anyhow::{Context, Result, bail};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const GUIDE: &str = include_str!("../templates/ai_docs.md");

pub fn add_to_file(name: &Path) -> Result<PathBuf> {
    add_to_directory(Path::new("."), name)
}

fn add_to_directory(directory: &Path, name: &Path) -> Result<PathBuf> {
    let destination = markdown_path(name)?;
    let destination = directory.join(destination);
    write_new_file(&destination)?;
    Ok(destination)
}

fn markdown_path(name: &Path) -> Result<PathBuf> {
    let Some(file_name) = name.file_name() else {
        bail!("AI guide filename must include a file name");
    };
    if name.as_os_str() != file_name {
        bail!("AI guide path must be a filename in the current directory");
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
        let path =
            super::add_to_directory(directory.path(), std::path::Path::new("assistant-guide"))
                .unwrap();

        assert_eq!(path.file_name().unwrap(), "assistant-guide.md");
        assert_eq!(fs::read_to_string(path).unwrap(), GUIDE);
    }

    #[test]
    fn preserves_a_markdown_extension_without_adding_a_second_one() {
        let directory = tempfile::tempdir().unwrap();
        let path =
            super::add_to_directory(directory.path(), std::path::Path::new("assistant-guide.md"))
                .unwrap();

        assert_eq!(path.file_name().unwrap(), "assistant-guide.md");
        assert_eq!(fs::read_to_string(path).unwrap(), GUIDE);
    }

    #[test]
    fn refuses_to_overwrite_an_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let existing = directory.path().join("assistant-guide.md");
        fs::write(&existing, "keep this file").unwrap();

        let error =
            super::add_to_directory(directory.path(), std::path::Path::new("assistant-guide"))
                .unwrap_err();

        assert!(error.to_string().contains("refusing to overwrite"));
        assert_eq!(fs::read_to_string(existing).unwrap(), "keep this file");
    }

    #[test]
    fn rejects_an_empty_filename() {
        let error = add_to_file(std::path::Path::new("")).unwrap_err();

        assert!(error.to_string().contains("must include a file name"));
    }

    #[test]
    fn rejects_a_path_instead_of_a_filename() {
        let directory = tempfile::tempdir().unwrap();
        let nested = directory.path().join("nested");
        fs::create_dir(&nested).unwrap();
        let destination = nested.join("assistant-guide.md");

        let error = super::add_to_directory(
            directory.path(),
            std::path::Path::new("nested/assistant-guide.md"),
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("must be a filename in the current directory")
        );
        assert!(!destination.exists());
    }

    #[test]
    fn rejects_trailing_directory_syntax() {
        let directory = tempfile::tempdir().unwrap();
        let nested = directory.path().join("nested");
        fs::create_dir(&nested).unwrap();

        for input in ["nested/", "nested/."] {
            let error =
                super::add_to_directory(directory.path(), std::path::Path::new(input)).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("must be a filename in the current directory")
            );
        }

        assert!(!nested.join(".md").exists());
    }
}
