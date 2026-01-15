//! FUSE read operation integration tests.
//!
//! Tests file listing and reading via `bale mount --shell`.

/// Verifies FUSE read operations work correctly.
#[test]
#[cfg_attr(not(feature = "integration-tests"), ignore = "requires FUSE")]
fn fuse_read_operations() {
    trycmd::TestCases::new()
        .case("tests/cmd/fuse_read/*.toml")
        .run();
}
