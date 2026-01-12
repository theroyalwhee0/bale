//! Shared proptest configuration for all property-based tests.

use proptest::prelude::ProptestConfig;

/// Returns the standard proptest configuration for this project.
///
/// Uses the default proptest configuration. Regression files are stored in
/// `proptest-regressions/` at the crate root (proptest's default location).
pub fn config() -> ProptestConfig {
    ProptestConfig::default()
}
