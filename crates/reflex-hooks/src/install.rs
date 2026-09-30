//! Install and uninstall, split into a pure planning step and an apply step.
//!
//! [`plan`] and [`plan_uninstall`] take the wizard's answers, a description of the
//! machine ([`Env`]) and the current contents of every file they may touch, and return
//! the list of [`FileChange`]s to make. They do no IO, so they are unit-tested with
//! in-memory files. [`apply`] performs the changes.
//!
//! Guarantees:
//! * Existing JSON keys and hooks are kept; only entries whose command starts with
//!   `reflex hook` are added, updated or removed.
//! * Re-running install with the same answers produces no changes.
//! * Files that install rewrites (agent settings, `.reflex.toml`) are copied to
//!   `<file>.reflex-bak` first, once; an existing backup is never overwritten.

use crate::adapters::{self, AgentAdapter};
use crate::config::{self, Config, Scope, PROJECT_CONFIG_NAME};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

/// Prefix that marks a hook command as ours in agent settings files.
pub const HOOK_COMMAND_PREFIX: &str = "reflex hook";

/// Entries added to `.gitignore` in project scope.
pub const GITIGNORE_ENTRIES: &[&str] = &[".reflex/", "*.reflex-bak"];

/// Current contents of the files planning may touch. A missing key means the file
/// does not exist.
pub type Files = BTreeMap<PathBuf, String>;

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("unknown agent `{0}`")]
    UnknownAgent(String),
    #[error("{agent} does not support {scope} scope")]
    UnsupportedScope { agent: String, scope: Scope },
    #[error("cannot find the home directory, needed for user scope")]
    NoHome,
    #[error("{0} is not inside a git repository")]
    NotGitRepo(PathBuf),
    #[error("{path} is not valid JSON ({message}); fix or remove it and run install again")]
    InvalidJson { path: PathBuf, message: String },
    #[error("{path}: {message}")]
    UnexpectedShape { path: PathBuf, message: String },
    #[error(transparent)]
    BadPattern(#[from] crate::guard::GuardError),
}

/// What planning needs to know about the machine.
#[derive(Debug, Clone, Default)]
pub struct Env {
    /// Project root (the git root, or the current directory outside a repository).
    pub root: PathBuf,
    pub home: Option<PathBuf>,
    /// Directory git runs hooks from (honours `core.hooksPath`); `None` outside a repo.
    pub git_hooks_dir: Option<PathBuf>,
    /// True if that directory is inside the working tree and not under `.git`, i.e. the
    /// hooks are files the whole team shares.
    pub hooks_dir_in_repo: bool,
}

impl Env {
    /// Path of the `.reflex.toml` the given scope reads and writes.
    pub fn config_path(&self, scope: Scope) -> Result<PathBuf, InstallError> {
        match scope {
            Scope::Project => Ok(self.root.join(PROJECT_CONFIG_NAME)),
            Scope::User => self
                .home
                .as_deref()
                .map(config::user_config_path_in)
                .ok_or(InstallError::NoHome),
        }
    }
}

/// The wizard's answers (or the flags of a non-interactive run).
#[derive(Debug, Clone)]
pub struct Answers {
    /// Adapter ids, e.g. `claude-code`, `git`.
    pub agents: Vec<String>,
    pub scope: Scope,
    /// Full settings to write to `.reflex.toml`; `agents` and `scope` are overwritten
    /// from the fields above.
    pub config: Config,
}

/// Everything an adapter needs to plan its files.
#[derive(Debug, Clone, Copy)]
pub struct PlanCtx<'a> {
    pub env: &'a Env,
    pub scope: Scope,
}

/// One file to create, modify or delete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: PathBuf,
    /// Current contents; `None` if the file does not exist yet.
    pub before: Option<String>,
    /// New contents; `None` deletes the file.
    pub after: Option<String>,
    /// Make the file executable (unix).
    pub executable: bool,
    /// Copy the file mode from this file before it is changed (unix).
    pub perm_from: Option<PathBuf>,
    /// Copy the old file to `<file>.reflex-bak` first, unless a backup already exists.
    pub backup: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Create,
    Modify,
    Delete,
}

impl FileChange {
    pub fn write(path: PathBuf, before: Option<&String>, after: String) -> FileChange {
        FileChange {
            path,
            before: before.cloned(),
            after: Some(after),
            executable: false,
            perm_from: None,
            backup: false,
        }
    }

    pub fn delete(path: PathBuf, before: &str) -> FileChange {
        FileChange {
            path,
            before: Some(before.to_string()),
            after: None,
            executable: false,
            perm_from: None,
            backup: false,
        }
    }

    pub fn kind(&self) -> ChangeKind {
        match (&self.before, &self.after) {
            (None, _) => ChangeKind::Create,
            (Some(_), None) => ChangeKind::Delete,
            (Some(_), Some(_)) => ChangeKind::Modify,
        }
    }

    /// One summary line: `+ path`, `~ path` or `- path`, with the path shortened
    /// relative to the project root or the home directory where possible.
    pub fn summary(&self, env: &Env) -> String {
        let sign = match self.kind() {
            ChangeKind::Create => '+',
            ChangeKind::Modify => '~',
            ChangeKind::Delete => '-',
        };
        let shown = if let Ok(rel) = self.path.strip_prefix(&env.root) {
            rel.display().to_string()
        } else if let Some(rel) = env
            .home
            .as_ref()
            .and_then(|h| self.path.strip_prefix(h).ok())
        {
            format!("~/{}", rel.display())
        } else {
            self.path.display().to_string()
        };
        format!("{sign} {shown}")
    }
}

/// Result of planning: the changes, plus things the user has to do by hand.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub changes: Vec<FileChange>,
    pub notes: Vec<String>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub fn extend(&mut self, other: Plan) {
        self.changes.extend(other.changes);
        self.notes.extend(other.notes);
    }
}

fn adapters_for(agents: &[String]) -> Result<Vec<&'static dyn AgentAdapter>, InstallError> {
    agents
        .iter()
        .map(|id| adapters::find(id).ok_or_else(|| InstallError::UnknownAgent(id.clone())))
        .collect()
}

/// Paths [`plan`] wants to read. Read them (missing files are simply absent) and pass
/// the result to [`plan`].
pub fn files_needed(answers: &Answers, env: &Env) -> Result<Vec<PathBuf>, InstallError> {
    let mut paths = vec![env.config_path(answers.scope)?];
    let ctx = PlanCtx {
        env,
        scope: answers.scope,
    };
    for a in adapters_for(&answers.agents)? {
        paths.extend(a.files(&ctx)?);
    }
    if answers.scope == Scope::Project {
        paths.push(env.root.join(".gitignore"));
    }
    Ok(paths)
}

/// Plans an install. Pure: `existing` holds the current contents of the files named
/// by [`files_needed`].
pub fn plan(answers: &Answers, env: &Env, existing: &Files) -> Result<Plan, InstallError> {
    let adapters = adapters_for(&answers.agents)?;
    for a in &adapters {
        if !a.scopes().contains(&answers.scope) {
            return Err(InstallError::UnsupportedScope {
                agent: a.id().to_string(),
                scope: answers.scope,
            });
        }
    }

    let mut out = Plan::default();

    // A pattern that does not compile would make every hook fail open at run time.
    crate::guard::PathPolicy::new(&answers.config.protect.paths, &env.root)?;

    // Settings file. It is only rewritten when it would parse to something different.
    let mut cfg = answers.config.clone();
    cfg.agents.enabled = answers.agents.clone();
    cfg.agents.scope = answers.scope;
    let cfg_path = env.config_path(answers.scope)?;
    let before = existing.get(&cfg_path);
    let unchanged = before
        .and_then(|t| Config::parse(t, &cfg_path).ok())
        .is_some_and(|c| c == cfg);
    if !unchanged {
        let mut change = FileChange::write(cfg_path, before, config::render(&cfg));
        change.backup = true;
        out.changes.push(change);
    }

    let ctx = PlanCtx {
        env,
        scope: answers.scope,
    };
    for a in &adapters {
        out.extend(a.plan_install(&ctx, existing)?);
    }

    if answers.scope == Scope::Project {
        let path = env.root.join(".gitignore");
        let before = existing.get(&path);
        if let Some(after) = gitignore_with_entries(before.map(String::as_str)) {
            out.changes.push(FileChange::write(path, before, after));
        }
    }
    Ok(out)
}

/// Paths [`plan_uninstall`] wants to read.
pub fn files_needed_uninstall(env: &Env) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for scope in [Scope::Project, Scope::User] {
        let ctx = PlanCtx { env, scope };
        for a in adapters::registry() {
            if a.scopes().contains(&scope) {
                if let Ok(files) = a.files(&ctx) {
                    paths.extend(files);
                }
            }
        }
    }
    paths
}

/// Plans an uninstall for every agent in both scopes. Only our own entries and files
/// are touched; `.reflex.toml` and `.gitignore` are left alone.
pub fn plan_uninstall(env: &Env, existing: &Files) -> Result<Plan, InstallError> {
    let mut out = Plan::default();
    for scope in [Scope::Project, Scope::User] {
        if scope == Scope::User && env.home.is_none() {
            continue;
        }
        let ctx = PlanCtx { env, scope };
        for a in adapters::registry() {
            if a.scopes().contains(&scope) {
                out.extend(a.plan_uninstall(&ctx, existing)?);
            }
        }
    }
    Ok(out)
}

/// Adds any missing [`GITIGNORE_ENTRIES`]. Returns the new text, or `None` if nothing
/// has to change.
pub fn gitignore_with_entries(before: Option<&str>) -> Option<String> {
    let text = before.unwrap_or("");
    let missing: Vec<&str> = GITIGNORE_ENTRIES
        .iter()
        .copied()
        .filter(|entry| {
            let bare = entry.trim_end_matches('/');
            !text.lines().any(|l| {
                let l = l.trim();
                l == *entry || l == bare || l == format!("/{entry}") || l == format!("/{bare}")
            })
        })
        .collect();
    if missing.is_empty() {
        return None;
    }
    let mut out = text.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str("# Reflex Control local state\n");
    for m in missing {
        out.push_str(m);
        out.push('\n');
    }
    Some(out)
}

/// Reads the given files; the ones that do not exist or are not UTF-8 are left out.
pub fn read_existing(paths: &[PathBuf]) -> Files {
    let mut files = Files::new();
    for p in paths {
        if let Ok(text) = std::fs::read_to_string(p) {
            files.insert(p.clone(), text);
        }
    }
    files
}

/// Writes the planned changes to disk, in order.
pub fn apply(changes: &[FileChange]) -> io::Result<()> {
    for change in changes {
        apply_one(change)?;
    }
    Ok(())
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".reflex-bak");
    path.with_file_name(name)
}

fn apply_one(change: &FileChange) -> io::Result<()> {
    #[cfg(unix)]
    let inherited_mode = change
        .perm_from
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .map(|m| m.permissions());

    if change.backup && change.path.is_file() {
        let bak = backup_path(&change.path);
        if !bak.exists() {
            std::fs::copy(&change.path, &bak)?;
        }
    }

    let Some(after) = &change.after else {
        return match std::fs::remove_file(&change.path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    };

    if let Some(parent) = change.path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&change.path, after)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Some(perm) = inherited_mode {
            std::fs::set_permissions(&change.path, perm)?;
        } else if change.executable {
            std::fs::set_permissions(&change.path, std::fs::Permissions::from_mode(0o755))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OnFailure;

    fn env(root: &Path) -> Env {
        Env {
            root: root.to_path_buf(),
            home: Some(root.join("home")),
            git_hooks_dir: Some(root.join(".git/hooks")),
            hooks_dir_in_repo: false,
        }
    }

    fn answers(agents: &[&str]) -> Answers {
        Answers {
            agents: agents.iter().map(|s| s.to_string()).collect(),
            scope: Scope::Project,
            config: Config::default(),
        }
    }

    /// Runs plan + apply against a real directory, the way the CLI does.
    fn install_on_disk(a: &Answers, e: &Env) -> Plan {
        let files = read_existing(&files_needed(a, e).unwrap());
        let p = plan(a, e, &files).unwrap();
        apply(&p.changes).unwrap();
        p
    }

    fn uninstall_on_disk(e: &Env) -> Plan {
        let files = read_existing(&files_needed_uninstall(e));
        let p = plan_uninstall(e, &files).unwrap();
        apply(&p.changes).unwrap();
        p
    }

    fn json(path: &Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn plan_on_empty_project_creates_config_settings_hook_and_gitignore() {
        let tmp = tempfile::tempdir().unwrap();
        let e = env(tmp.path());
        let p = plan(&answers(&["claude-code", "git"]), &e, &Files::new()).unwrap();
        let paths: Vec<PathBuf> = p.changes.iter().map(|c| c.path.clone()).collect();
        assert_eq!(
            paths,
            vec![
                tmp.path().join(".reflex.toml"),
                tmp.path().join(".claude/settings.json"),
                tmp.path().join(".git/hooks/pre-commit"),
                tmp.path().join(".gitignore"),
            ]
        );
        assert!(p.changes.iter().all(|c| c.kind() == ChangeKind::Create));
        assert!(p.notes.is_empty());

        let settings: serde_json::Value =
            serde_json::from_str(p.changes[1].after.as_deref().unwrap()).unwrap();
        let pre = &settings["hooks"]["PreToolUse"][0];
        assert_eq!(pre["matcher"], "Edit|Write|MultiEdit|NotebookEdit|Bash");
        assert_eq!(
            pre["hooks"][0]["command"],
            "reflex hook claude-code pre-tool"
        );
        assert_eq!(pre["hooks"][0]["timeout"], 10);
        let stop = &settings["hooks"]["Stop"][0];
        assert_eq!(stop["hooks"][0]["command"], "reflex hook claude-code stop");
        assert_eq!(stop["hooks"][0]["timeout"], 600);

        let hook = &p.changes[2];
        assert!(hook.executable);
        assert!(hook
            .after
            .as_deref()
            .unwrap()
            .contains("reflex hook git pre-commit"));

        let ignore = p.changes[3].after.as_deref().unwrap();
        assert!(ignore.contains(".reflex/"));
    }

    #[test]
    fn config_file_reflects_the_answers() {
        let tmp = tempfile::tempdir().unwrap();
        let e = env(tmp.path());
        let mut a = answers(&["claude-code"]);
        a.config.tests.command = "npm test".into();
        a.config.tests.on_failure = OnFailure::Ask;
        let p = plan(&a, &e, &Files::new()).unwrap();
        let text = p.changes[0].after.as_deref().unwrap();
        let cfg = Config::parse(text, Path::new(".reflex.toml")).unwrap();
        assert_eq!(cfg.tests.command, "npm test");
        assert_eq!(cfg.tests.on_failure, OnFailure::Ask);
        assert_eq!(cfg.agents.enabled, ["claude-code"]);
    }

    #[test]
    fn merging_keeps_unrelated_keys_and_hooks() {
        let tmp = tempfile::tempdir().unwrap();
        let e = env(tmp.path());
        let original = serde_json::json!({
            "model": "opus",
            "permissions": { "allow": ["Bash(ls:*)"], "deny": [] },
            "hooks": {
                "PreToolUse": [
                    { "matcher": "Bash", "hooks": [
                        { "type": "command", "command": "./scripts/audit.sh", "timeout": 5 }
                    ]}
                ],
                "PostToolUse": [
                    { "matcher": "Edit", "hooks": [{ "type": "command", "command": "prettier --write" }] }
                ],
                "Stop": [
                    { "hooks": [{ "type": "command", "command": "notify-send done" }] }
                ]
            },
            "env": { "FOO": "1" }
        });
        let path = tmp.path().join(".claude/settings.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_string_pretty(&original).unwrap()).unwrap();

        install_on_disk(&answers(&["claude-code"]), &e);
        let merged = json(&path);

        // Unrelated keys are untouched.
        assert_eq!(merged["model"], original["model"]);
        assert_eq!(merged["permissions"], original["permissions"]);
        assert_eq!(merged["env"], original["env"]);
        assert_eq!(
            merged["hooks"]["PostToolUse"],
            original["hooks"]["PostToolUse"]
        );
        // Existing hooks come first, ours are appended as their own group.
        let pre = merged["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 2);
        assert_eq!(pre[0], original["hooks"]["PreToolUse"][0]);
        assert_eq!(
            pre[1]["hooks"][0]["command"],
            "reflex hook claude-code pre-tool"
        );
        let stop = merged["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2);
        assert_eq!(stop[0], original["hooks"]["Stop"][0]);
        assert_eq!(
            stop[1]["hooks"][0]["command"],
            "reflex hook claude-code stop"
        );

        // The original file was backed up once.
        let bak = tmp.path().join(".claude/settings.json.reflex-bak");
        assert_eq!(json(&bak), original);
    }

    #[test]
    fn key_order_of_existing_settings_is_preserved() {
        let tmp = tempfile::tempdir().unwrap();
        let e = env(tmp.path());
        let path = tmp.path().join(".claude/settings.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{\n  \"zeta\": 1,\n  \"alpha\": 2\n}\n").unwrap();
        install_on_disk(&answers(&["claude-code"]), &e);
        let text = std::fs::read_to_string(&path).unwrap();
        let z = text.find("\"zeta\"").unwrap();
        let a = text.find("\"alpha\"").unwrap();
        let h = text.find("\"hooks\"").unwrap();
        assert!(z < a && a < h, "{text}");
    }

    #[test]
    fn install_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let e = env(tmp.path());
        std::fs::create_dir_all(tmp.path().join(".git/hooks")).unwrap();
        let a = answers(&["claude-code", "git"]);
        let first = install_on_disk(&a, &e);
        assert!(!first.is_empty());

        let files = read_existing(&files_needed(&a, &e).unwrap());
        let second = plan(&a, &e, &files).unwrap();
        assert!(second.is_empty(), "second plan: {:?}", second.changes);

        // No duplicate hook entries after a second full run either.
        install_on_disk(&a, &e);
        let settings = json(&tmp.path().join(".claude/settings.json"));
        assert_eq!(settings["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
        assert_eq!(settings["hooks"]["Stop"].as_array().unwrap().len(), 1);
        let ignore = std::fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        assert_eq!(ignore.matches(".reflex/").count(), 1);
    }

    #[test]
    fn changed_answers_update_our_entries_in_place() {
        let tmp = tempfile::tempdir().unwrap();
        let e = env(tmp.path());
        let mut a = answers(&["claude-code"]);
        install_on_disk(&a, &e);
        a.config.tests.command = "pytest".into();
        let p = install_on_disk(&a, &e);
        // Only the config file changes; the settings file is already right.
        assert_eq!(p.changes.len(), 1);
        assert_eq!(p.changes[0].path, tmp.path().join(".reflex.toml"));
        let cfg = Config::load_file(&tmp.path().join(".reflex.toml")).unwrap();
        assert_eq!(cfg.tests.command, "pytest");
    }

    #[test]
    fn uninstall_restores_the_original_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let e = env(tmp.path());
        let original = serde_json::json!({
            "model": "opus",
            "hooks": {
                "PreToolUse": [
                    { "matcher": "Bash", "hooks": [{ "type": "command", "command": "./audit.sh" }] }
                ]
            }
        });
        let path = tmp.path().join(".claude/settings.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&original).unwrap() + "\n",
        )
        .unwrap();
        let before_text = std::fs::read_to_string(&path).unwrap();

        install_on_disk(&answers(&["claude-code"]), &e);
        assert_ne!(json(&path), original);
        uninstall_on_disk(&e);
        assert_eq!(json(&path), original);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before_text);

        // A second uninstall has nothing to do.
        let files = read_existing(&files_needed_uninstall(&e));
        assert!(plan_uninstall(&e, &files).unwrap().is_empty());
    }

    #[test]
    fn uninstall_removes_files_that_install_created() {
        let tmp = tempfile::tempdir().unwrap();
        let e = env(tmp.path());
        std::fs::create_dir_all(tmp.path().join(".git/hooks")).unwrap();
        install_on_disk(&answers(&["claude-code", "git"]), &e);
        assert!(tmp.path().join(".claude/settings.json").is_file());
        assert!(tmp.path().join(".git/hooks/pre-commit").is_file());

        uninstall_on_disk(&e);
        assert!(!tmp.path().join(".claude/settings.json").exists());
        assert!(!tmp.path().join(".git/hooks/pre-commit").exists());
        // The user's own settings file survives.
        assert!(tmp.path().join(".reflex.toml").is_file());
    }

    #[test]
    fn foreign_pre_commit_hook_is_chained_and_restored() {
        let tmp = tempfile::tempdir().unwrap();
        let e = env(tmp.path());
        let hooks = tmp.path().join(".git/hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        let foreign = "#!/bin/sh\necho lint\nexit 0\n";
        std::fs::write(hooks.join("pre-commit"), foreign).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                hooks.join("pre-commit"),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }

        let a = answers(&["git"]);
        let p = install_on_disk(&a, &e);
        assert!(p.notes.is_empty());
        // The old hook moved to pre-commit.local and ours calls it first.
        assert_eq!(
            std::fs::read_to_string(hooks.join("pre-commit.local")).unwrap(),
            foreign
        );
        let ours = std::fs::read_to_string(hooks.join("pre-commit")).unwrap();
        assert!(ours.contains("pre-commit.local"));
        assert!(ours.contains("reflex hook git pre-commit"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |n: &str| {
                std::fs::metadata(hooks.join(n))
                    .unwrap()
                    .permissions()
                    .mode()
            };
            assert_eq!(mode("pre-commit") & 0o111, 0o111);
            assert_eq!(mode("pre-commit.local") & 0o111, 0o111);
        }

        // Re-running does not move our own script over the saved one.
        let files = read_existing(&files_needed(&a, &e).unwrap());
        assert!(plan(&a, &e, &files).unwrap().is_empty());

        uninstall_on_disk(&e);
        assert_eq!(
            std::fs::read_to_string(hooks.join("pre-commit")).unwrap(),
            foreign
        );
        assert!(!hooks.join("pre-commit.local").exists());
    }

    #[test]
    fn foreign_hook_with_existing_local_file_is_left_alone_with_a_note() {
        let tmp = tempfile::tempdir().unwrap();
        let e = env(tmp.path());
        let hooks = tmp.path().join(".git/hooks");
        let mut files = Files::new();
        files.insert(hooks.join("pre-commit"), "#!/bin/sh\necho a\n".into());
        files.insert(hooks.join("pre-commit.local"), "#!/bin/sh\necho b\n".into());
        let p = plan(&answers(&["git"]), &e, &files).unwrap();
        assert!(!p.changes.iter().any(|c| c.path.starts_with(&hooks)));
        assert_eq!(p.notes.len(), 1);
        assert!(p.notes[0].contains("reflex hook git pre-commit"));
    }

    #[test]
    fn shared_hooks_directory_is_not_modified() {
        let tmp = tempfile::tempdir().unwrap();
        let mut e = env(tmp.path());
        e.git_hooks_dir = Some(tmp.path().join(".githooks"));
        e.hooks_dir_in_repo = true;
        let p = plan(&answers(&["git"]), &e, &Files::new()).unwrap();
        assert!(!p
            .changes
            .iter()
            .any(|c| c.path.starts_with(tmp.path().join(".githooks"))));
        assert!(p.notes[0].contains("reflex hook git pre-commit"));
    }

    #[test]
    fn user_scope_writes_home_files_and_rejects_git() {
        let tmp = tempfile::tempdir().unwrap();
        let e = env(tmp.path());
        let mut a = answers(&["claude-code"]);
        a.scope = Scope::User;
        let p = plan(&a, &e, &Files::new()).unwrap();
        let paths: Vec<PathBuf> = p.changes.iter().map(|c| c.path.clone()).collect();
        assert_eq!(
            paths,
            vec![
                tmp.path().join("home/.config/reflex/reflex.toml"),
                tmp.path().join("home/.claude/settings.json"),
            ]
        );

        let mut g = answers(&["git"]);
        g.scope = Scope::User;
        assert!(matches!(
            plan(&g, &e, &Files::new()),
            Err(InstallError::UnsupportedScope { .. })
        ));
    }

    #[test]
    fn invalid_settings_json_is_an_error_not_an_overwrite() {
        let tmp = tempfile::tempdir().unwrap();
        let e = env(tmp.path());
        let mut files = Files::new();
        files.insert(
            tmp.path().join(".claude/settings.json"),
            "{ not json".into(),
        );
        let err = plan(&answers(&["claude-code"]), &e, &files).unwrap_err();
        assert!(matches!(err, InstallError::InvalidJson { .. }));
    }

    #[test]
    fn bad_protect_pattern_is_rejected_before_anything_is_written() {
        let tmp = tempfile::tempdir().unwrap();
        let mut a = answers(&["claude-code"]);
        a.config.protect.paths.push("[".into());
        let err = plan(&a, &env(tmp.path()), &Files::new()).unwrap_err();
        assert!(matches!(err, InstallError::BadPattern(_)));
        assert!(err.to_string().contains("protect.paths"));
    }

    #[test]
    fn unknown_agent_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let err = plan(&answers(&["nope"]), &env(tmp.path()), &Files::new()).unwrap_err();
        assert!(matches!(err, InstallError::UnknownAgent(_)));
    }

    #[test]
    fn gitignore_edit_is_minimal_and_idempotent() {
        let after = gitignore_with_entries(Some("target/\n")).unwrap();
        assert!(after.starts_with("target/\n"));
        assert!(after.contains(".reflex/\n"));
        assert_eq!(gitignore_with_entries(Some(&after)), None);
        // Already ignored in another spelling.
        assert_eq!(
            gitignore_with_entries(Some("/.reflex\n*.reflex-bak\n")),
            None
        );
        // No trailing newline in the original.
        assert!(gitignore_with_entries(Some("target"))
            .unwrap()
            .starts_with("target\n"));
    }

    #[test]
    fn backup_is_written_once() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a.json");
        std::fs::write(&path, "one").unwrap();
        let mut c = FileChange::write(path.clone(), Some(&"one".to_string()), "two".into());
        c.backup = true;
        apply(&[c]).unwrap();
        let mut c = FileChange::write(path.clone(), Some(&"two".to_string()), "three".into());
        c.backup = true;
        apply(&[c]).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "three");
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("a.json.reflex-bak")).unwrap(),
            "one"
        );
    }
}
