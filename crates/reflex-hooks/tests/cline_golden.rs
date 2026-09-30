//! Golden tests for the cline adapter; see `common/mod.rs` for the fixture format.

mod common;

#[test]
fn cline_fixtures() {
    common::run_fixtures("cline", "cline", 19);
}
