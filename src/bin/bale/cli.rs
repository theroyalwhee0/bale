//! Command-line interface definitions.

use std::path::PathBuf;

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
pub struct Cli {
    /// The subcommand to run.
    #[command(subcommand)]
    pub command: Command,
}

/// Available subcommands.
#[derive(Subcommand)]
pub enum Command {
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
