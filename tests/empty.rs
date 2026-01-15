//! Integration tests for empty bale archives using external CLI tools.

/// Verifies empty.bale with file, zipinfo, and unzip.
#[test]
#[cfg_attr(not(feature = "integration-tests"), ignore = "requires external tools")]
fn cli_tools() {
    trycmd::TestCases::new()
        .case("tests/cmd/empty_archive/*.toml")
        .run();
}
