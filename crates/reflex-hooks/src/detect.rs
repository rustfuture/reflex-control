//! Detection of agents and of the project's test command.

use crate::adapters::{self, Probe};
use std::path::{Path, PathBuf};

/// Ids of the adapters that seem to be in use in `root`.
pub fn detect_agents(root: &Path, on_path: &dyn Fn(&str) -> bool) -> Vec<&'static str> {
    let probe = Probe { root, on_path };
    adapters::registry()
        .iter()
        .filter(|a| a.detect(&probe))
        .map(|a| a.id())
        .collect()
}

/// Finds `program` in the `PATH` directories.
pub fn find_on_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };
    std::env::split_paths(&path).find_map(|dir| {
        exts.iter()
            .map(|ext| dir.join(format!("{program}{ext}")))
            .find(|candidate| candidate.is_file())
    })
}

/// True if `program` can be found on `PATH`.
pub fn program_on_path(program: &str) -> bool {
    find_on_path(program).is_some()
}

/// Guesses the command that runs the project's tests. Empty if nothing is recognised.
pub fn detect_test_command(root: &Path) -> String {
    if root.join("Cargo.toml").is_file() {
        return "cargo test".to_string();
    }
    if npm_has_test_script(&root.join("package.json")) {
        return "npm test".to_string();
    }
    if root.join("pyproject.toml").is_file() || root.join("pytest.ini").is_file() {
        return "pytest".to_string();
    }
    if root.join("go.mod").is_file() {
        return "go test ./...".to_string();
    }
    String::new()
}

fn npm_has_test_script(path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    match pkg["scripts"]["test"].as_str() {
        // `npm init` writes a placeholder test script that always fails.
        Some(script) => !script.trim().is_empty() && !script.contains("no test specified"),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir_with(files: &[(&str, &str)]) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        for (name, body) in files {
            std::fs::write(tmp.path().join(name), body).unwrap();
        }
        tmp
    }

    #[test]
    fn test_command_detection() {
        let cases: Vec<(Vec<(&str, &str)>, &str)> = vec![
            (vec![("Cargo.toml", "")], "cargo test"),
            (
                vec![("package.json", r#"{"scripts":{"test":"jest"}}"#)],
                "npm test",
            ),
            (
                vec![(
                    "package.json",
                    r#"{"scripts":{"test":"echo \"Error: no test specified\" && exit 1"}}"#,
                )],
                "",
            ),
            (vec![("package.json", r#"{"name":"x"}"#)], ""),
            (vec![("pyproject.toml", "")], "pytest"),
            (vec![("pytest.ini", "")], "pytest"),
            (vec![("go.mod", "module x")], "go test ./..."),
            (vec![], ""),
            // Cargo wins over the rest in a mixed repository.
            (
                vec![
                    ("Cargo.toml", ""),
                    ("package.json", r#"{"scripts":{"test":"x"}}"#),
                ],
                "cargo test",
            ),
        ];
        for (files, expected) in cases {
            let tmp = dir_with(&files);
            assert_eq!(detect_test_command(tmp.path()), expected, "{files:?}");
        }
    }

    #[test]
    fn agent_detection_uses_the_project_and_path() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(detect_agents(tmp.path(), &|_| false).is_empty());

        std::fs::create_dir(tmp.path().join(".git")).unwrap();
        assert_eq!(detect_agents(tmp.path(), &|_| false), ["git"]);

        assert_eq!(
            detect_agents(tmp.path(), &|p| p == "claude"),
            ["claude-code", "git"]
        );

        std::fs::create_dir(tmp.path().join(".claude")).unwrap();
        assert_eq!(
            detect_agents(tmp.path(), &|_| false),
            ["claude-code", "git"]
        );
    }
}
