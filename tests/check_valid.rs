//! Tests for check command on valid archives.

#[test]
fn cli_tools() {
    trycmd::TestCases::new().case("tests/cmd/check_valid/*.toml");
}
