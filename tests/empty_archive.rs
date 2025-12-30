//! Integration tests for empty bale archives using external CLI tools.

#[test]
fn cli_tools() {
    trycmd::TestCases::new()
        .case("tests/cmd/empty_archive/*.toml")
        .run();
}
