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
        /// Path prefix for files in the archive.
        #[arg(long, default_value = "")]
        prefix: String,
        /// Files to add to the archive.
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// List entries in a bale archive.
    List {
        /// The archive to list.
        archive: PathBuf,
    },
    /// Extract entries from a bale archive.
    Extract {
        /// The archive to extract from.
        archive: PathBuf,
        /// Output directory (defaults to current directory).
        #[arg(short, long, default_value = ".")]
        output: PathBuf,
        /// Entries to extract (if empty, extracts all).
        entries: Vec<String>,
    },
    /// Delete entries from a bale archive.
    Delete {
        /// The archive to modify.
        archive: PathBuf,
        /// Entries to delete.
        #[arg(required = true)]
        entries: Vec<String>,
    },
    /// Compact a bale archive, removing orphaned data.
    Compact {
        /// The archive to compact.
        archive: PathBuf,
    },
    /// Check archive integrity.
    Check {
        /// The archive to check.
        archive: PathBuf,
        /// Attempt to fix issues found (sorts CD, renames duplicates).
        #[arg(long)]
        fix: bool,
        /// Repair CRC mismatches by recomputing checksums.
        #[arg(long)]
        fix_crc: bool,
    },
}
