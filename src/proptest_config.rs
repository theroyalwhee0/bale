//! Shared proptest configuration for all property-based tests.

use proptest::prelude::ProptestConfig;
use proptest::test_runner::FileFailurePersistence;

/// Returns the standard proptest configuration for this project.
///
/// Regression files are stored in `tests/proptest-regressions/` using an
/// absolute path derived from `CARGO_MANIFEST_DIR` at compile time.
pub fn config() -> ProptestConfig {
    ProptestConfig {
        failure_persistence: Some(Box::new(FileFailurePersistence::SourceParallel(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/proptest-regressions"
        )))),
        ..ProptestConfig::default()
    }
}
