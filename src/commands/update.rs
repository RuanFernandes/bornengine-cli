use anyhow::Result;
use serde_json::Value;
use std::time::Duration;

const RELEASES_URL: &str =
    "https://api.github.com/repos/RuanFernandes/bornengine-cli/releases/latest";
const INSTALL_COMMAND: &str =
    "cargo install --git https://github.com/RuanFernandes/bornengine-cli --force";

pub fn check() -> Result<i32> {
    let current = env!("CARGO_PKG_VERSION");
    println!("BornEngine CLI: {current}");
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .into();
    match agent
        .get(RELEASES_URL)
        .header("User-Agent", "bornengine-cli")
        .call()
    {
        Ok(response) => {
            let release = response.into_body().read_json::<Value>();
            match release.ok().and_then(|value| {
                value
                    .get("tag_name")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            }) {
                Some(tag) => match semver::Version::parse(tag.trim_start_matches('v')) {
                    Ok(latest) => {
                        let current_version = semver::Version::parse(current)?;
                        if latest > current_version {
                            println!(
                                "Version {latest} is available. Update with:\n  {INSTALL_COMMAND}"
                            );
                        } else {
                            println!("You are using the latest published CLI release.");
                        }
                    }
                    Err(_) => println!(
                        "The latest GitHub release has an invalid version tag; install with:\n  {INSTALL_COMMAND}"
                    ),
                },
                None => println!(
                    "Could not read the latest release tag; install with:\n  {INSTALL_COMMAND}"
                ),
            }
        }
        Err(ureq::Error::StatusCode(404)) => println!(
            "No CLI release is published yet. When one is available, update with:\n  {INSTALL_COMMAND}"
        ),
        Err(error) => println!(
            "Could not check GitHub releases ({error}). Update manually with:\n  {INSTALL_COMMAND}"
        ),
    }
    Ok(0)
}
