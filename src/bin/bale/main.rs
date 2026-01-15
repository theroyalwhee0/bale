//! Bale CLI tool for working with bale archives.
//!
//! # Commands
//!
//! - `touch` - Create an empty archive
//! - `add` - Add files to an archive
//! - `ls` (alias: `list`) - List archive contents
//! - `extract` - Extract files from an archive
//! - `delete` - Remove entries from an archive
//! - `compact` - Remove orphaned data and duplicates (requires `compact` feature)
//! - `check` - Validate archive integrity
//! - `mount` - Mount archive as FUSE filesystem (requires `fuse` feature)
//!
//! # Features
//!
//! - `cli` - Base CLI (no compact, no fuse)
//! - `compact` - Adds compact command
//! - `fuse` - Adds mount command (includes cli)
//! - `bin` - Full binary (includes compact + fuse)
//!
//! # Usage
//!
//! ```text
//! bale touch archive.bale
//! bale add archive.bale file1.txt file2.txt
//! bale ls archive.bale
//! bale extract archive.bale -o output_dir
//! bale compact archive.bale
//! bale mount archive.bale /mnt/archive
//! ```

mod cli;
mod commands;
mod error;

use std::process::ExitCode;

use clap::Parser;

use cli::{Cli, Command};
use error::BaleCliError;

/// Entry point for the bale CLI.
///
/// # Exit Codes
///
/// - `0` - Success
/// - `1` - Failure (I/O errors, invalid archives, missing files, etc.)
///
/// All errors are printed to stderr before exiting.
fn main() -> ExitCode {
    env_logger::init();

    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // CLI tools legitimately print errors to stderr.
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
        // Archive creation.
        Command::Touch { path, path_size } => commands::touch::run(path, path_size),

        // Content modification.
        Command::Add {
            archive,
            prefix,
            files,
        } => commands::add::run(archive, &prefix, &files),
        Command::Delete {
            archive,
            ignore_missing,
            entries,
        } => commands::delete::run(archive, &entries, ignore_missing),

        // Content access.
        Command::List { archive } => commands::list::run(archive),
        Command::Extract {
            archive,
            output,
            entries,
        } => commands::extract::run(archive, output, &entries),

        // Maintenance.
        #[cfg(feature = "compact")]
        Command::Compact { archive } => commands::compact::run(archive),
        Command::Check { archive, quiet } => commands::check::run(archive, quiet),

        // Filesystem.
        #[cfg(feature = "fuse")]
        Command::Mount {
            archive,
            mount_point,
            background,
            allow_root,
            allow_other,
            shell,
            read_only,
        } => commands::mount::run(
            archive,
            mount_point,
            background,
            allow_root,
            allow_other,
            shell,
            read_only,
        ),
    }
}
