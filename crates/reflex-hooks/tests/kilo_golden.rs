//! Golden tests for the kilo adapter; see `common/mod.rs` for the fixture format.

mod common;

#[test]
fn kilo_fixtures() {
    common::run_fixtures("kilo", "kilo", 16);
}
