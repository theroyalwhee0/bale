//! Mount a bale archive as a FUSE filesystem.

use std::path::PathBuf;
use std::process::Command;

use bale::fuse::BaleFs;
use nix::unistd::daemon;

use crate::error::BaleCliError;

/// Runs the mount command.
///
/// # Errors
///
/// Returns an error if:
/// - The archive cannot be opened
/// - The mount point is invalid
/// - FUSE mounting fails
/// - Shell mode fails to spawn or execute
pub fn run(
    archive: PathBuf,
    mount_point: Option<PathBuf>,
    background: bool,
    allow_root: bool,
    allow_other: bool,
    shell: Option<Option<String>>,
    read_only: bool,
) -> Result<(), BaleCliError> {
    // Background mode is incompatible with shell mode.
    if background && shell.is_some() {
        return Err(BaleCliError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "--background cannot be used with --shell",
        )));
    }

    if background {
        let mount_point = mount_point.ok_or_else(|| {
            BaleCliError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "mount point required with --background",
            ))
        })?;
        return run_background_mode(archive, mount_point, allow_root, allow_other, read_only);
    }

    if let Some(script) = shell {
        run_shell_mode(archive, allow_root, allow_other, script, read_only)
    } else {
        let mount_point = mount_point.ok_or_else(|| {
            BaleCliError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "mount point required (or use --shell)",
            ))
        })?;
        run_standard_mode(archive, mount_point, allow_root, allow_other, read_only)
    }
}

/// Standard foreground mount mode.
fn run_standard_mode(
    archive: PathBuf,
    mount_point: PathBuf,
    allow_root: bool,
    allow_other: bool,
    read_only: bool,
) -> Result<(), BaleCliError> {
    let fs = BaleFs::new(&archive, read_only)?;
    let session = fs.mount(&mount_point, allow_root, allow_other)?;
    session.join();
    Ok(())
}

/// Background (daemon) mount mode.
///
/// Validates the archive, prints the PID, daemonizes, then mounts.
fn run_background_mode(
    archive: PathBuf,
    mount_point: PathBuf,
    allow_root: bool,
    allow_other: bool,
    read_only: bool,
) -> Result<(), BaleCliError> {
    // Validate archive before daemonizing so errors are reported to user.
    let fs = BaleFs::new(&archive, read_only)?;

    // Print PID before daemonizing (child will have different PID).
    #[allow(clippy::print_stderr)]
    {
        eprintln!("Daemonizing with PID {}", std::process::id());
    }

    // Daemonize: fork to background, create new session, close std fds.
    // nochdir=false: change to /
    // noclose=false: redirect stdin/stdout/stderr to /dev/null
    daemon(false, false).map_err(|e| {
        BaleCliError::Io(std::io::Error::other(format!("failed to daemonize: {e}")))
    })?;

    // Now running in background - mount and wait.
    let session = fs.mount(&mount_point, allow_root, allow_other)?;
    session.join();
    Ok(())
}

/// Shell mode: mount to temp dir, spawn shell, cleanup on exit.
fn run_shell_mode(
    archive: PathBuf,
    allow_root: bool,
    allow_other: bool,
    script: Option<String>,
    read_only: bool,
) -> Result<(), BaleCliError> {
    // Create temp directory (auto-cleaned on drop).
    let temp_dir = tempfile::Builder::new().prefix("bale-").tempdir()?;

    // Mount filesystem.
    let fs = BaleFs::new(&archive, read_only)?;
    let session = fs.mount(temp_dir.path(), allow_root, allow_other)?;

    // Get shell from $SHELL or fallback to /bin/sh.
    let shell_path = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());

    // Build and run shell command.
    let mut cmd = Command::new(&shell_path);
    cmd.current_dir(temp_dir.path());
    cmd.env("BALE_MOUNT", temp_dir.path());

    if let Some(script) = script {
        cmd.args(["-c", &script]);
    }

    // CLI tools legitimately print status to stderr.
    #[allow(clippy::print_stderr)]
    {
        eprintln!("Starting shell at {}", temp_dir.path().display());
    }

    let status = cmd.status()?;

    // Cleanup: drop session first (unmounts), then temp_dir cleans up.
    drop(session);
    drop(temp_dir);

    if !status.success()
        && let Some(code) = status.code()
    {
        std::process::exit(code);
    }

    Ok(())
}
