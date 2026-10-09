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
    }
    Ok(0)
}
