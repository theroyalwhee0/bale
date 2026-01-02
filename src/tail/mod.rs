//! Unified 256-byte archive tail (trailer) structures.
//!
//! This module contains all trailer-related structures:
//! - [`Zip64Eocd`] - ZIP64 End of Central Directory record (56 bytes)
//! - [`Zip64EocdLocator`] - ZIP64 EOCD Locator (20 bytes)
//! - [`Eocd`] - Standard End of Central Directory record (22 bytes)
//! - [`BaleEocd`] - Bale-specific EOCD extension (158 bytes)
//! - [`Trailer`] - Unified 256-byte trailer combining all structures

/// Bale-specific EOCD extension.
mod bale_eocd;
/// Standard End of Central Directory record.
mod eocd;
/// Unified trailer combining all structures.
mod trailer;
/// ZIP64 End of Central Directory record.
mod zip64_eocd;
/// ZIP64 EOCD Locator.
mod zip64_eocd_locator;

pub use bale_eocd::BaleEocd;
pub use eocd::Eocd;
pub use trailer::Trailer;
pub use zip64_eocd::Zip64Eocd;
pub use zip64_eocd_locator::Zip64EocdLocator;
