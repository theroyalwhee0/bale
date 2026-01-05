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
    /// Create an empty bale archive or update its modification time.
    Touch {
        /// The archive to create or touch.
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
    #[command(visible_alias = "list")]
    Ls {
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
        /// Don't error if entries are not found.
        #[arg(long)]
        ignore_missing: bool,
        /// Entries to delete.
        #[arg(required = true)]
        entries: Vec<String>,
    },
    /// Compact a bale archive, removing orphaned data.
    #[cfg(feature = "compact")]
    Compact {
        /// The archive to compact.
        archive: PathBuf,
    },
    /// Check archive integrity.
    Check {
        /// The archive to check.
        archive: PathBuf,
        /// Suppress output (exit code only).
        #[arg(short, long)]
        quiet: bool,
    },
    /// Mount a bale archive as a filesystem.
    #[cfg(feature = "fuse")]
    #[command(visible_alias = "mnt")]
    Mount {
        /// The archive to mount.
        archive: PathBuf,
        /// The mount point directory.
        mount_point: PathBuf,
        /// Run in background (daemonize).
        #[arg(short, long)]
        background: bool,
        /// Allow root to access the mount.
        #[arg(long)]
        allow_root: bool,
        /// Allow other users to access the mount.
        #[arg(long)]
        allow_other: bool,
        /// Spawn a shell at the mount point (optionally with a command).
        #[arg(long)]
        shell: Option<Option<String>>,
    },
}
