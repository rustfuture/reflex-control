//! Helpers shared by the adapters whose integration is a single file that Reflex owns
//! (a plugin, an extension or a hook script), plus a few input-reading helpers.
//!
//! An agent that merges entries into a shared settings file (Claude Code) does its own
//! planning. An agent that loads one file per feature gets the same rules here:
//! * the file carries [`MARKER`] in its header, which is how install, uninstall and
//!   `doctor` tell it from a file of the user's with the same name
//! * a file of ours is created or brought up to date; a file that is not ours is never
//!   touched, and the plan says what to add by hand instead
//! * uninstall deletes the file only if it is ours

use super::codex::resolve;
use super::HookEvent;
use crate::install::{FileChange, Files, Plan};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Present in the header of every file we write.
pub const MARKER: &str = "reflex-control managed hook";

pub fn is_ours(text: &str) -> bool {
    text.contains(MARKER)
}

/// One file Reflex owns.
pub struct ManagedFile {
    pub path: PathBuf,
    pub contents: String,
    /// Make the file executable (unix).
    pub executable: bool,
}

impl ManagedFile {
    /// Plans the install. `by_hand` tells the user what to do if a file of theirs is in
    /// the way.
    pub fn plan_install(&self, existing: &Files, by_hand: &str) -> Plan {
        let mut plan = Plan::default();
        let before = existing.get(&self.path);
        match before {
            Some(text) if !is_ours(text) => plan.notes.push(format!(
                "{} already exists and is not managed by Reflex Control, so it was not changed. {by_hand}",
                self.path.display()
            )),
            Some(text) if *text == self.contents => {}
            _ => {
                let mut change =
                    FileChange::write(self.path.clone(), before, self.contents.clone());
                change.executable = self.executable;
                plan.changes.push(change);
            }
        }
        plan
    }
}

/// Deletes the file at `path` if it is ours.
pub fn plan_uninstall(path: &Path, existing: &Files) -> Plan {
    let mut plan = Plan::default();
    if let Some(text) = existing.get(path).filter(|t| is_ours(t)) {
        plan.changes
            .push(FileChange::delete(path.to_path_buf(), text));
    }
    plan
}

pub fn is_installed(path: &Path, existing: &Files) -> bool {
    existing.get(path).is_some_and(|t| is_ours(t))
}

/// The non-empty string under the first of `keys` that holds one.
pub fn string_field(input: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        input
            .get(key)?
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    })
}

/// Makes the paths of a file-writing event absolute against `cwd`, the way the agent
/// resolves them: `secrets/a` written from `app/` is `app/secrets/a`, which the
/// root-relative pattern `secrets/**` must not match.
pub fn resolve_paths(event: HookEvent, cwd: Option<&str>) -> HookEvent {
    match event {
        HookEvent::PreWrite { paths } => HookEvent::PreWrite {
            paths: paths.into_iter().map(|p| resolve(cwd, p)).collect(),
        },
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_are_resolved_against_the_agents_directory() {
        let write = |p: &str| HookEvent::PreWrite {
            paths: vec![p.to_string()],
        };
        assert_eq!(
            resolve_paths(write("secrets/a"), Some("/work/proj/app")),
            write("/work/proj/app/secrets/a")
        );
        assert_eq!(resolve_paths(write("/etc/x"), Some("/w")), write("/etc/x"));
        assert_eq!(resolve_paths(write("a"), None), write("a"));
        let shell = HookEvent::PreShell {
            command: "ls".into(),
        };
        assert_eq!(resolve_paths(shell.clone(), Some("/w")), shell);
    }

    #[test]
    fn string_field_takes_the_first_key_that_holds_text() {
        let v = serde_json::json!({ "a": "", "b": 3, "c": "x", "d": "y" });
        assert_eq!(
            string_field(&v, &["a", "b", "c", "d"]).as_deref(),
            Some("x")
        );
        assert_eq!(string_field(&v, &["a", "b"]), None);
    }

    #[test]
    fn managed_file_rules() {
        let path = PathBuf::from("/p/reflex.js");
        let file = ManagedFile {
            path: path.clone(),
            contents: format!("// {MARKER}\nnew\n"),
            executable: false,
        };
        let mut files = Files::new();

        // Missing: created.
        let p = file.plan_install(&files, "by hand");
        assert_eq!(p.changes.len(), 1);
        assert!(p.notes.is_empty());

        // Same contents: nothing to do.
        files.insert(path.clone(), file.contents.clone());
        assert!(file.plan_install(&files, "by hand").is_empty());

        // Older version of ours: updated.
        files.insert(path.clone(), format!("// {MARKER}\nold\n"));
        assert_eq!(file.plan_install(&files, "by hand").changes.len(), 1);

        // The user's own file: untouched, with a note.
        files.insert(path.clone(), "mine\n".into());
        let p = file.plan_install(&files, "by hand");
        assert!(p.changes.is_empty());
        assert!(p.notes[0].contains("by hand"));
        assert!(plan_uninstall(&path, &files).is_empty());
        assert!(!is_installed(&path, &files));

        // Uninstall removes ours.
        files.insert(path.clone(), file.contents.clone());
        assert!(is_installed(&path, &files));
        assert_eq!(plan_uninstall(&path, &files).changes.len(), 1);
    }
}
