//! Bale CLI tool for working with bale archives.

use std::path::PathBuf;

use bale::Archive;
use clap::{Parser, Subcommand};

/// Command-line interface for bale.
#[derive(Parser)]
#[command(
    version = concat!(
        env!("CARGO_PKG_VERSION"),
        "\n",
        "  Built:   ",
        env!("BUILD_DATETIME_ISO"),
        "\n",
        "  Profile: ",
        env!("EXPECT_PROFILE"),
    ),
    about
)]
struct Cli {
    /// The subcommand to run.
    #[command(subcommand)]
    command: Command,
}

/// Available subcommands.
#[derive(Subcommand)]
enum Command {
    /// Create a file or update its modification time.
    Touch {
        /// The file to touch.
        path: PathBuf,
    },
    /// Add files to a bale archive.
    Add {
        /// The archive to add files to.
        archive: PathBuf,
        /// Files to add to the archive.
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
}

/// Entry point for the bale CLI.
#[allow(clippy::print_stderr)]
fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Command::Touch { path } => bale::touch(path),
        Command::Add { archive, files } => add_files(archive, files),
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

/// Adds files to an archive.
fn add_files(archive_path: PathBuf, files: Vec<PathBuf>) -> Result<(), bale::BaleError> {
    let mut archive = Archive::create(archive_path)?;

    for file in &files {
        let name = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unnamed");
        archive.add_file(file, name)?;
    }

    archive.finish()
}
