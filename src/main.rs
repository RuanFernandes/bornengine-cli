use bornengine_cli::cli::Cli;
use clap::Parser;

fn main() {
    let cli = Cli::parse();
    match bornengine_cli::commands::execute(cli) {
        Ok(0) => {}
        Ok(exit_code) => std::process::exit(exit_code),
        Err(error) => {
            eprintln!("Error: {error:#}");
            std::process::exit(1);
        }
    }
}
