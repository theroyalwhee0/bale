//! FUSE read operation integration tests.
//!
//! Tests file listing and reading via `bale mount --shell`.

/// Verifies FUSE read operations work correctly.
#[test]
fn fuse_read_operations() {
    trycmd::TestCases::new()
        .case("tests/cmd/fuse_read/*.toml")
        .run();
}
