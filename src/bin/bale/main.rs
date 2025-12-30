//! Bale CLI tool for working with bale archives.

mod cli;
mod commands;
mod error;

use clap::Parser;

use cli::{Cli, Command};

/// Entry point for the bale CLI.
#[allow(clippy::print_stderr)]
fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Command::Touch { path } => commands::touch::run(path),
        Command::Add { archive, files } => commands::add::run(archive, &files),
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
