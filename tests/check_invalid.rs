//! Tests for check command on invalid archives.

#[test]
fn cli_tools() {
    trycmd::TestCases::new().case("tests/cmd/check_invalid/*.toml");
}
