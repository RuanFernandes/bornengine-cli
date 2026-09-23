use crate::cli::ConfigCommands;
use crate::config::Config;
use anyhow::Result;

pub fn execute(command: ConfigCommands) -> Result<i32> {
    let path = Config::path()?;
    let mut config = Config::load(&path)?;
    match command {
        ConfigCommands::Set { key, value } => {
            config.set(&key, &value)?;
            config.save(&path)?;
            println!("Set {key} = {value}");
        }
        ConfigCommands::Get { key } => match config.get(&key) {
            Some(value) => println!("{value}"),
            None => anyhow::bail!("unsupported configuration key `{key}`"),
        },
        ConfigCommands::List => {
            for (key, value) in config.entries() {
                println!("{key} = {value}");
            }
        }
    }
    Ok(0)
}
