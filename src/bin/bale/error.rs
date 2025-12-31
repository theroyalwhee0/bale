//! CLI-specific error types.

use std::io;
use std::str::Utf8Error;
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
    /// Invalid UTF-8.
    #[error(transparent)]
    Utf8(#[from] Utf8Error),
}
