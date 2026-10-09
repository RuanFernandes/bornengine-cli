use crate::cli::PerryCommands;
use crate::perry;
use anyhow::Result;

pub fn execute(command: PerryCommands) -> Result<i32> {
    match command {
        PerryCommands::Install { release } => {
            let installed = perry::install(release.as_deref())?;
            println!(
                "Installed Perry {} at {}",
                installed.release,
                installed.program.display()
            );
            if let Some(commit) = installed.commit {
                println!("Perry commit: {commit}");
            }
        }
        PerryCommands::Path => println!("{}", perry::program()),
        PerryCommands::List => {
            let releases = perry::installed_releases()?;
            if releases.is_empty() {
                println!("No Perry compilers installed. Run `bornengine perry install`.");
            }
            for release in releases {
                let marker = if release.current { " (current)" } else { "" };
                println!("{}{marker}", release.tag);
            }
        }
        PerryCommands::Clean { dry_run } => {
            let removed = perry::clean(dry_run)?;
            if removed.is_empty() {
                println!("Nothing to remove.");
            }
            let verb = if dry_run { "Would remove" } else { "Removed" };
            for path in removed {
                println!("{verb} {}", path.display());
            }
        }
    }
    Ok(0)
}
