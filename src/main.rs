use bornengine_cli::cli::Cli;
use bornengine_cli::ui::{self, Tone};
use clap::Parser;

fn main() {
    if let Some(exit_code) = bornengine_cli::cargo_profile::run_cargo_proxy_if_requested() {
        std::process::exit(exit_code);
    }
    let cli = Cli::parse();
    match bornengine_cli::commands::execute(cli) {
        Ok(0) => {}
        Ok(exit_code) => std::process::exit(exit_code),
        Err(error) => {
            eprintln!("{} {error:#}", ui::paint_stderr("Error:", Tone::Error));
            std::process::exit(1);
        }
    }
}
