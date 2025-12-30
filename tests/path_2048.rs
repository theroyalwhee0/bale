//! Integration tests for bale archives with 2048-byte path size.

/// Verifies path_2048.bale with zipinfo.
#[test]
fn cli_tools() {
    trycmd::TestCases::new()
        .case("tests/cmd/path_2048/*.toml")
        .run();
}
