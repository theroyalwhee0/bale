//! Integration tests for bale archives with 16KB alignment.

/// Verifies align_16k.bale with zipinfo.
#[test]
#[cfg_attr(not(feature = "integration-tests"), ignore = "requires external tools")]
fn cli_tools() {
    trycmd::TestCases::new()
        .case("tests/cmd/align_16k/*.toml")
        .run();
}
