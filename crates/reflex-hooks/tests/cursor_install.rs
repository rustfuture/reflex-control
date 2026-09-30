//! Install and uninstall planning for the Cursor adapter, against real directories.

use reflex_hooks::adapters::{self, Probe};
use reflex_hooks::config::{Config, Scope};
use reflex_hooks::install::{
    apply, files_needed, files_needed_uninstall, plan, plan_uninstall, read_existing, Answers,
    ChangeKind, Env, Files, InstallError, Plan, PlanCtx,
};
use serde_json::{json, Value};
use std::path::Path;

fn env(root: &Path) -> Env {
    Env {
        root: root.to_path_buf(),
        home: Some(root.join("home")),
        git_hooks_dir: Some(root.join(".git/hooks")),
        hooks_dir_in_repo: false,
    }
}

fn answers(scope: Scope) -> Answers {
    Answers {
        agents: vec!["cursor".to_string()],
        scope,
        config: Config::default(),
    }
}

/// Runs plan + apply against a real directory, the way the CLI does.
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

fn json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn empty_project_gets_config_hooks_file_and_gitignore() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path());
    let p = plan(&answers(Scope::Project), &e, &Files::new()).unwrap();
    let paths: Vec<_> = p.changes.iter().map(|c| c.path.clone()).collect();
    assert_eq!(
        paths,
        vec![
            tmp.path().join(".reflex.toml"),
            tmp.path().join(".cursor/hooks.json"),
            tmp.path().join(".gitignore"),
        ]
    );
    assert!(p.changes.iter().all(|c| c.kind() == ChangeKind::Create));

    let hooks: Value = serde_json::from_str(p.changes[1].after.as_deref().unwrap()).unwrap();
    assert_eq!(
        hooks,
        json!({
            "version": 1,
            "hooks": {
                "preToolUse": [{
                    "command": "reflex hook cursor pre-tool",
                    "matcher": "Write|Delete",
                    "timeout": 10
                }],
                "beforeShellExecution": [{
                    "command": "reflex hook cursor shell",
                    "timeout": 10
                }],
                "stop": [{
                    "command": "reflex hook cursor stop",
                    "timeout": 600
                }]
            }
        })
    );
    // `version` comes first, as in Cursor's own examples.
    let text = p.changes[1].after.as_deref().unwrap();
    assert!(text.find("\"version\"").unwrap() < text.find("\"hooks\"").unwrap());
    assert!(p.notes.is_empty());
}

#[test]
fn merging_keeps_foreign_hooks_and_keys() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path());
    let original = json!({
        "version": 1,
        "hooks": {
            "beforeShellExecution": [
                { "command": "./scripts/audit.sh", "timeout": 5 }
            ],
            "afterFileEdit": [
                { "command": "prettier --write" }
            ],
            "stop": [
                { "command": "notify-send done", "loop_limit": 3 }
            ]
        }
    });
    let path = tmp.path().join(".cursor/hooks.json");
    write(&path, &serde_json::to_string_pretty(&original).unwrap());

    install(&answers(Scope::Project), &e);
    let merged = json(&path);

    assert_eq!(merged["version"], 1);
    assert_eq!(
        merged["hooks"]["afterFileEdit"],
        original["hooks"]["afterFileEdit"]
    );
    // Foreign entries stay first and untouched; ours are appended.
    let shell = merged["hooks"]["beforeShellExecution"].as_array().unwrap();
    assert_eq!(shell.len(), 2);
    assert_eq!(shell[0], original["hooks"]["beforeShellExecution"][0]);
    assert_eq!(shell[1]["command"], "reflex hook cursor shell");
    let stop = merged["hooks"]["stop"].as_array().unwrap();
    assert_eq!(stop.len(), 2);
    assert_eq!(stop[0], original["hooks"]["stop"][0]);
    assert_eq!(stop[1]["command"], "reflex hook cursor stop");
    assert_eq!(merged["hooks"]["preToolUse"].as_array().unwrap().len(), 1);

    // The original file was backed up once.
    assert_eq!(
        json(&tmp.path().join(".cursor/hooks.json.reflex-bak")),
        original
    );
}

#[test]
fn a_file_without_version_gets_one_and_nothing_else_moves() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path());
    let path = tmp.path().join(".cursor/hooks.json");
    write(&path, "{\n  \"hooks\": {}\n}\n");
    install(&answers(Scope::Project), &e);
    assert_eq!(json(&path)["version"], 1);
    uninstall(&e);
    // Only the version key we added is left, so the file goes.
    assert!(!path.exists());
}

#[test]
fn install_is_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path());
    let a = answers(Scope::Project);
    let first = install(&a, &e);
    assert!(!first.is_empty());

    let files = read_existing(&files_needed(&a, &e).unwrap());
    let second = plan(&a, &e, &files).unwrap();
    assert!(second.is_empty(), "second plan: {:?}", second.changes);

    install(&a, &e);
    let hooks = json(&tmp.path().join(".cursor/hooks.json"));
    for event in ["preToolUse", "beforeShellExecution", "stop"] {
        assert_eq!(
            hooks["hooks"][event].as_array().unwrap().len(),
            1,
            "{event}"
        );
    }
}

#[test]
fn outdated_and_duplicate_entries_of_ours_are_collapsed() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path());
    let path = tmp.path().join(".cursor/hooks.json");
    write(
        &path,
        &json!({
            "version": 1,
            "hooks": { "preToolUse": [
                { "command": "reflex hook cursor pre-tool", "matcher": "Shell", "timeout": 1 },
                { "command": "./mine.sh" },
                { "command": "reflex hook cursor pre-tool" }
            ]}
        })
        .to_string(),
    );
    install(&answers(Scope::Project), &e);
    let hooks = json(&path);
    let pre = hooks["hooks"]["preToolUse"].as_array().unwrap();
    assert_eq!(pre.len(), 2);
    assert_eq!(pre[0]["matcher"], "Write|Delete");
    assert_eq!(pre[0]["timeout"], 10);
    assert_eq!(pre[1]["command"], "./mine.sh");
}

#[test]
fn uninstall_restores_the_original_file() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path());
    let original = json!({
        "version": 1,
        "hooks": {
            "beforeShellExecution": [{ "command": "./audit.sh" }]
        }
    });
    let path = tmp.path().join(".cursor/hooks.json");
    write(
        &path,
        &(serde_json::to_string_pretty(&original).unwrap() + "\n"),
    );
    let before_text = std::fs::read_to_string(&path).unwrap();

    install(&answers(Scope::Project), &e);
    assert_ne!(json(&path), original);
    uninstall(&e);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before_text);

    let files = read_existing(&files_needed_uninstall(&e));
    assert!(plan_uninstall(&e, &files).unwrap().is_empty());
}

#[test]
fn uninstall_deletes_a_file_that_install_created() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path());
    install(&answers(Scope::Project), &e);
    assert!(tmp.path().join(".cursor/hooks.json").is_file());
    uninstall(&e);
    assert!(!tmp.path().join(".cursor/hooks.json").exists());
}

#[test]
fn user_scope_writes_under_the_home_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path());
    let a = answers(Scope::User);
    assert_eq!(
        files_needed(&a, &e).unwrap(),
        vec![
            tmp.path().join("home/.config/reflex/reflex.toml"),
            tmp.path().join("home/.cursor/hooks.json"),
        ]
    );
    install(&a, &e);
    assert!(tmp.path().join("home/.cursor/hooks.json").is_file());
    assert!(!tmp.path().join(".cursor").exists());
    uninstall(&e);
    assert!(!tmp.path().join("home/.cursor/hooks.json").exists());
}

#[test]
fn invalid_json_is_an_error_not_an_overwrite() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path());
    let mut files = Files::new();
    files.insert(tmp.path().join(".cursor/hooks.json"), "{ not json".into());
    let err = plan(&answers(Scope::Project), &e, &files).unwrap_err();
    assert!(matches!(err, InstallError::InvalidJson { .. }));

    files.insert(
        tmp.path().join(".cursor/hooks.json"),
        r#"{"hooks": []}"#.into(),
    );
    let err = plan(&answers(Scope::Project), &e, &files).unwrap_err();
    assert!(matches!(err, InstallError::UnexpectedShape { .. }));
}

#[test]
fn doctor_sees_the_install() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path());
    let adapter = adapters::find("cursor").unwrap();
    let ctx = PlanCtx {
        env: &e,
        scope: Scope::Project,
    };
    let files = read_existing(&files_needed_uninstall(&e));
    assert!(!adapter.is_installed(&ctx, &files));
    install(&answers(Scope::Project), &e);
    let files = read_existing(&files_needed_uninstall(&e));
    assert!(adapter.is_installed(&ctx, &files));

    // A hooks file with only the stop hook is not a full install.
    let mut half = Files::new();
    half.insert(
        tmp.path().join(".cursor/hooks.json"),
        json!({"version": 1, "hooks": {"stop": [{"command": "reflex hook cursor stop"}]}})
            .to_string(),
    );
    assert!(!adapter.is_installed(&ctx, &half));
}

#[test]
fn detection_uses_the_directory_or_either_binary() {
    let tmp = tempfile::tempdir().unwrap();
    let adapter = adapters::find("cursor").unwrap();
    let probe = |on_path: &dyn Fn(&str) -> bool| {
        adapter.detect(&Probe {
            root: tmp.path(),
            on_path,
        })
    };
    assert!(!probe(&|_| false));
    assert!(probe(&|p| p == "cursor"));
    assert!(probe(&|p| p == "cursor-agent"));
    assert!(!probe(&|p| p == "codex"));
    std::fs::create_dir(tmp.path().join(".cursor")).unwrap();
    assert!(probe(&|_| false));
}
