//! Golden tests for the pi adapter; see `common/mod.rs` for the fixture format.

mod common;

#[test]
fn pi_fixtures() {
    common::run_fixtures("pi", "pi", 16);
}
