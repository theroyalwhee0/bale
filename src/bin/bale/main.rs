//! Bale CLI tool for working with bale archives.

mod cli;
mod commands;
mod error;

use std::process::ExitCode;

use clap::Parser;

use cli::{Cli, Command};
use error::BaleCliError;

/// Entry point for the bale CLI.
fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            #[allow(clippy::print_stderr)]
            {
                eprintln!("error: {e}");
            }
            ExitCode::FAILURE
        }
    }
}

/// Runs the CLI with the given arguments.
fn run(cli: Cli) -> Result<(), BaleCliError> {
    match cli.command {
        Command::Touch { path } => commands::touch::run(path),
        Command::Add { archive, files } => commands::add::run(archive, &files),
    }
}
