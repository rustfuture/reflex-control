//! Runs the installed pre-commit script with a stand-in `reflex` to check the shell
//! logic: only exit code 1 blocks, a saved hook runs first, a missing binary is fine.

#![cfg(unix)]

use reflex_hooks::adapters::git;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

/// Executing a file that another thread has only just written can fail with ETXTBSY
/// when a parallel test forks in between, so these tests run one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

fn write_exec(path: &Path, body: &str) {
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

struct Setup {
    _tmp: tempfile::TempDir,
    hooks: std::path::PathBuf,
    bin: std::path::PathBuf,
}

fn setup(reflex_exit: Option<i32>) -> Setup {
    let tmp = tempfile::tempdir().unwrap();
    let hooks = tmp.path().join("hooks");
    let bin = tmp.path().join("bin");
    std::fs::create_dir_all(&hooks).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    write_exec(&hooks.join("pre-commit"), git::script());
    if let Some(code) = reflex_exit {
        write_exec(
            &bin.join("reflex"),
            &format!(
                "#!/bin/sh\necho \"reflex $*\" >> \"{}/calls\"\nexit {code}\n",
                tmp.path().display()
            ),
        );
    }
    Setup {
        _tmp: tmp,
        hooks,
        bin,
    }
}

fn run(s: &Setup) -> i32 {
    let path = format!("{}:/usr/bin:/bin", s.bin.display());
    Command::new(s.hooks.join("pre-commit"))
        .env("PATH", path)
        .status()
        .unwrap()
        .code()
        .unwrap()
}

#[test]
fn exit_code_one_blocks_and_everything_else_allows() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(run(&setup(Some(0))), 0);
    assert_eq!(run(&setup(Some(1))), 1);
    // A crash or any other failure of reflex itself must not block the commit.
    assert_eq!(run(&setup(Some(101))), 0);
    assert_eq!(run(&setup(Some(2))), 0);
}

#[test]
fn missing_reflex_binary_allows_the_commit() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(run(&setup(None)), 0);
}

#[test]
fn script_calls_the_git_hook_command() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let s = setup(Some(0));
    run(&s);
    let calls = std::fs::read_to_string(s.hooks.parent().unwrap().join("calls")).unwrap();
    assert_eq!(calls.trim(), "reflex hook git pre-commit");
}

#[test]
fn saved_hook_runs_first_and_can_block() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let s = setup(Some(0));
    write_exec(&s.hooks.join("pre-commit.local"), "#!/bin/sh\nexit 7\n");
    assert_eq!(run(&s), 7);
    // Reflex was not reached.
    assert!(!s.hooks.parent().unwrap().join("calls").exists());

    write_exec(&s.hooks.join("pre-commit.local"), "#!/bin/sh\nexit 0\n");
    assert_eq!(run(&s), 0);
    assert!(s.hooks.parent().unwrap().join("calls").exists());
}
