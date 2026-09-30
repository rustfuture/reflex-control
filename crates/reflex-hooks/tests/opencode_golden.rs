//! Golden tests for the opencode adapter; see `common/mod.rs` for the fixture format.

mod common;

#[test]
fn opencode_fixtures() {
    common::run_fixtures("opencode", "opencode", 19);
}
