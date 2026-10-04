use std::fs;
use std::process::Command;

#[test]
fn add_ai_docs_appends_markdown_and_writes_the_guide() {
    let directory = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_bornengine"))
        .current_dir(directory.path())
        .args(["--add-ai-docs", "assistant-guide"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let guide = fs::read_to_string(directory.path().join("assistant-guide.md")).unwrap();
    assert!(guide.starts_with("# BornEngine — AI context reference for language models"));
    assert!(guide.contains("Do not pass `Game` to assets."));
    assert!(guide.contains("AStarGrid2D"));
    assert!(guide.contains("SeededRandom"));
    assert!(guide.contains("BLOOM_NO_HOT_RELOAD=1"));
}

#[test]
fn add_ai_docs_keeps_an_existing_markdown_extension_and_never_overwrites() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("assistant-guide.md");
    fs::write(&path, "keep this file").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_bornengine"))
        .current_dir(directory.path())
        .args(["--add-ai-docs", "assistant-guide.md"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("refusing to overwrite"),
        "unexpected CLI error: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(path).unwrap(), "keep this file");
}
