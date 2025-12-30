//! Fixture generation tests.
//!
//! Run with `cargo test --test fixtures -- --ignored` to regenerate fixtures.

use std::fs::File;
use std::io::Write;
use std::path::Path;

use bale::Eocd;
use zerocopy::IntoBytes;

const FIXTURES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

#[test]
#[ignore]
fn generate_empty_bale() {
    let path = Path::new(FIXTURES_DIR).join("empty.bale");
    let mut file = File::create(&path).expect("failed to create empty.bale");

    let eocd = Eocd::empty();
    file.write_all(eocd.as_bytes())
        .expect("failed to write EOCD");
}
