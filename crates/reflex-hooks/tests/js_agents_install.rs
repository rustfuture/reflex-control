//! Install and uninstall for OpenCode, Kilo Code, Cline and pi, on real directories:
//! what is written, that a file of the user's is never touched, that installing twice
//! changes nothing, and that the embedded scripts are valid.

use reflex_hooks::adapters::{self, cline, opencode, pi};
use reflex_hooks::config::{Config, Scope};
use reflex_hooks::install::{
    apply, files_needed, files_needed_uninstall, plan, plan_uninstall, read_existing, Answers, Env,
    Plan, PlanCtx,
};
use std::path::{Path, PathBuf};
use std::process::Command;

fn env(root: &Path) -> Env {
    Env {
        root: root.to_path_buf(),
        home: Some(root.join("home")),
        git_hooks_dir: Some(root.join(".git/hooks")),
        hooks_dir_in_repo: false,
    }
}

fn answers(agents: &[&str], scope: Scope) -> Answers {
    Answers {
        agents: agents.iter().map(|s| s.to_string()).collect(),
        scope,
        config: Config::default(),
    }
}

fn install(a: &Answers, e: &Env) -> Plan {
    let files = read_existing(&files_needed(a, e).unwrap());
    let p = plan(a, e, &files).unwrap();
    apply(&p.changes).unwrap();
    p
}

fn uninstall(e: &Env) -> Plan {
    let files = read_existing(&files_needed_uninstall(e));
    let p = plan_uninstall(e, &files).unwrap();
    apply(&p.changes).unwrap();
    p
}

/// What `reflex install` writes for one agent, relative to the project root, and where
/// the file the agent loads is.
struct Case {
    id: &'static str,
    project_file: &'static str,
    user_file: &'static str,
    /// A sibling in the same directory that belongs to the user.
    sibling: &'static str,
    expected: fn() -> String,
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            id: "opencode",
            project_file: ".opencode/plugins/reflex.js",
            user_file: "home/.config/opencode/plugins/reflex.js",
            sibling: "other.js",
            expected: || opencode::plugin_source("opencode"),
        },
        Case {
            id: "kilo",
            project_file: ".kilo/plugin/reflex.js",
            user_file: "home/.config/kilo/plugin/reflex.js",
            sibling: "other.js",
            expected: || opencode::plugin_source("kilo"),
        },
        Case {
            id: "cline",
            project_file: if cfg!(windows) {
                ".clinerules/hooks/PreToolUse.ps1"
            } else {
                ".clinerules/hooks/PreToolUse"
            },
            user_file: if cfg!(windows) {
                "home/Documents/Cline/Hooks/PreToolUse.ps1"
            } else {
                "home/Documents/Cline/Hooks/PreToolUse"
            },
            sibling: "PostToolUse",
            expected: || {
                if cfg!(windows) {
                    cline::SCRIPT_WINDOWS.to_string()
                } else {
                    cline::SCRIPT_UNIX.to_string()
                }
            },
        },
        Case {
            id: "pi",
            project_file: ".pi/extensions/reflex.ts",
            user_file: "home/.pi/agent/extensions/reflex.ts",
            sibling: "other.ts",
            expected: || pi::EXTENSION.to_string(),
        },
    ]
}

fn ctx_installed(id: &str, e: &Env, scope: Scope) -> bool {
    let files = read_existing(&files_needed_uninstall(e));
    adapters::find(id)
        .unwrap()
        .is_installed(&PlanCtx { env: e, scope }, &files)
}

#[test]
fn install_into_an_empty_project_writes_the_embedded_file() {
    for case in cases() {
        let tmp = tempfile::tempdir().unwrap();
        let e = env(tmp.path());
        let p = install(&answers(&[case.id], Scope::Project), &e);

        let path = tmp.path().join(case.project_file);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            (case.expected)(),
            "{}",
            case.id
        );
        let written: Vec<&PathBuf> = p.changes.iter().map(|c| &c.path).collect();
        assert!(written.contains(&&path), "{}: {written:?}", case.id);
        assert!(ctx_installed(case.id, &e, Scope::Project), "{}", case.id);
        assert!(!ctx_installed(case.id, &e, Scope::User), "{}", case.id);

        // Only Cline needs a manual step, and it says so.
        assert_eq!(
            p.notes.len(),
            usize::from(case.id == "cline"),
            "{}",
            case.id
        );

        #[cfg(unix)]
        if case.id == "cline" {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "hook must be executable");
        }
    }
}

#[test]
fn user_scope_writes_below_the_home_directory() {
    for case in cases() {
        let tmp = tempfile::tempdir().unwrap();
        let e = env(tmp.path());
        install(&answers(&[case.id], Scope::User), &e);
        assert_eq!(
            std::fs::read_to_string(tmp.path().join(case.user_file)).unwrap(),
            (case.expected)(),
            "{}",
            case.id
        );
        assert!(ctx_installed(case.id, &e, Scope::User), "{}", case.id);
        assert!(!tmp.path().join(case.project_file).exists(), "{}", case.id);
    }
}

#[test]
fn installing_twice_changes_nothing() {
    for case in cases() {
        let tmp = tempfile::tempdir().unwrap();
        let e = env(tmp.path());
        let a = answers(&[case.id], Scope::Project);
        install(&a, &e);
        let again = install(&a, &e);
        assert!(again.is_empty(), "{}: {:?}", case.id, again.changes);
        assert!(again.notes.is_empty(), "{}", case.id);
    }
}

#[test]
fn an_older_copy_of_ours_is_brought_up_to_date() {
    for case in cases() {
        let tmp = tempfile::tempdir().unwrap();
        let e = env(tmp.path());
        let path = tmp.path().join(case.project_file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "// reflex-control managed hook\nold version\n").unwrap();

        install(&answers(&[case.id], Scope::Project), &e);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            (case.expected)(),
            "{}",
            case.id
        );
    }
}

#[test]
fn a_file_of_the_users_is_kept_and_reported() {
    for case in cases() {
        let tmp = tempfile::tempdir().unwrap();
        let e = env(tmp.path());
        let path = tmp.path().join(case.project_file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "my own hook\n").unwrap();

        let p = install(&answers(&[case.id], Scope::Project), &e);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "my own hook\n");
        assert_eq!(p.notes.len(), 1, "{}: {:?}", case.id, p.notes);
        assert!(p.notes[0].contains("not managed by Reflex Control"));
        assert!(p.notes[0].contains("reflex hook"), "{}", p.notes[0]);
        assert!(!ctx_installed(case.id, &e, Scope::Project), "{}", case.id);

        // The config was still written, and uninstall leaves the file alone.
        assert!(tmp.path().join(".reflex.toml").is_file());
        let removed = uninstall(&e);
        assert!(removed.is_empty(), "{}", case.id);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "my own hook\n");
    }
}

#[test]
fn uninstall_removes_only_our_file() {
    for case in cases() {
        let tmp = tempfile::tempdir().unwrap();
        let e = env(tmp.path());
        let ours = tmp.path().join(case.project_file);
        let sibling = ours.with_file_name(case.sibling);
        std::fs::create_dir_all(ours.parent().unwrap()).unwrap();
        std::fs::write(&sibling, "not ours\n").unwrap();

        install(&answers(&[case.id], Scope::Project), &e);
        assert!(ours.is_file());

        let p = uninstall(&e);
        assert_eq!(p.changes.len(), 1, "{}", case.id);
        assert!(!ours.exists(), "{}", case.id);
        assert_eq!(std::fs::read_to_string(&sibling).unwrap(), "not ours\n");
        assert!(!ctx_installed(case.id, &e, Scope::Project), "{}", case.id);
        assert!(uninstall(&e).is_empty(), "{}", case.id);
    }
}

#[test]
fn all_agents_together_do_not_share_files() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path());
    let ids: Vec<&str> = cases().iter().map(|c| c.id).collect();
    let a = answers(&ids, Scope::Project);
    let p = install(&a, &e);
    let mut paths: Vec<&PathBuf> = p.changes.iter().map(|c| &c.path).collect();
    let n = paths.len();
    paths.sort();
    paths.dedup();
    assert_eq!(paths.len(), n);
    assert!(install(&a, &e).is_empty());
    assert_eq!(uninstall(&e).changes.len(), 4);
}

#[test]
fn embedded_files_are_marked_and_the_plugins_have_the_agent_id() {
    for text in [
        opencode::plugin_source("opencode"),
        opencode::plugin_source("kilo"),
        pi::EXTENSION.to_string(),
        cline::SCRIPT_UNIX.to_string(),
        cline::SCRIPT_WINDOWS.to_string(),
    ] {
        assert!(text.contains("reflex-control managed hook"));
        assert!(!text.contains("__AGENT__"));
    }
    assert!(opencode::plugin_source("kilo").contains(r#"const AGENT = "kilo""#));
    assert!(pi::EXTENSION.contains(r#""hook", "pi""#));
    assert!(cline::SCRIPT_UNIX.contains("reflex hook cline pre-tool"));
    assert!(cline::SCRIPT_WINDOWS.contains("reflex hook cline pre-tool"));
}

/// Runs `program args`; `None` if the program is not installed.
fn try_run(program: &str, args: &[&Path]) -> Option<std::process::Output> {
    Command::new(program).args(args).output().ok()
}

#[test]
fn embedded_scripts_pass_a_syntax_check() {
    let tmp = tempfile::tempdir().unwrap();

    // The plugins are ES modules; node only treats a file as one if it ends in .mjs.
    for (name, text) in [
        ("opencode.mjs", opencode::plugin_source("opencode")),
        ("kilo.mjs", opencode::plugin_source("kilo")),
        ("pi.mjs", pi::EXTENSION.to_string()),
    ] {
        let path = tmp.path().join(name);
        std::fs::write(&path, text).unwrap();
        let Some(out) = try_run("node", &[Path::new("--check"), &path]) else {
            eprintln!("node is not installed; skipping the JS syntax check");
            break;
        };
        assert!(
            out.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let script = tmp.path().join("PreToolUse");
    std::fs::write(&script, cline::SCRIPT_UNIX).unwrap();
    match try_run("sh", &[Path::new("-n"), &script]) {
        Some(out) => assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        ),
        None => eprintln!("sh is not installed; skipping the shell syntax check"),
    }
}
