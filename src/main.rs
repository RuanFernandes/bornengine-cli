use bornengine_cli::cli::Cli;
use bornengine_cli::ui::{self, Tone};
use clap::Parser;

fn main() {
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
