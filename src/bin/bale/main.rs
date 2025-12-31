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
        Command::Add {
            archive,
            prefix,
            files,
        } => commands::add::run(archive, &prefix, &files),
        Command::List { archive } => commands::list::run(archive),
        Command::Extract {
            archive,
            output,
            entries,
        } => commands::extract::run(archive, output, &entries),
        Command::Delete { archive, entries } => commands::delete::run(archive, &entries),
        Command::Compact { archive } => commands::compact::run(archive),
        Command::Check { archive } => commands::check::run(archive),
    }
}
