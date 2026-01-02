//! CLI-specific error types.

use std::io;
use thiserror::Error;

/// CLI-specific error type.
#[derive(Error, Debug)]
pub enum BaleCliError {
    /// Error from the bale library.
    #[error(transparent)]
    Bale(#[from] bale::BaleError),
    /// I/O error.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// Archive check found issues.
    #[error("check failed: {0} issue(s) found")]
    CheckFailed(usize),
}
