//! Integration tests for single-file bale archives using external CLI tools.

/// Verifies single_file.bale with file, zipinfo, and unzip.
#[test]
fn cli_tools() {
    trycmd::TestCases::new()
        .case("tests/cmd/single_file/*.toml")
        .run();
}
