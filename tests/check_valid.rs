//! Tests for check command on valid archives.

#[test]
#[ignore = "v2 format refactor in progress"]
fn cli_tools() {
    trycmd::TestCases::new().case("tests/cmd/check_valid/*.toml");
}
