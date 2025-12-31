//! Tests for check command --fix flag using trycmd.

#[test]
fn cli_tools() {
    trycmd::TestCases::new().case("tests/cmd/check_fix/*.toml");
}
