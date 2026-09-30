//! `.reflex.toml`: the settings the hooks read at run time and the installer writes.
//!
//! Every key is optional. A missing file or a missing key falls back to the defaults
//! below, so an empty file and no file behave the same.
//!
//! Lookup order (see [`Config::discover`]):
//! 1. `.reflex.toml` in the current directory or the nearest parent, up to the repo root
//! 2. `~/.config/reflex/reflex.toml`
//! 3. built-in defaults

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};

pub const PROJECT_CONFIG_NAME: &str = ".reflex.toml";

/// Paths that agents may not write to unless the project's config says otherwise.
pub const DEFAULT_PROTECTED_PATHS: &[&str] = &[
    ".env",
    ".env.*",
    "*.pem",
    "*.key",
    "secrets/**",
    ".github/workflows/**",
    "migrations/**",
];

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid {path}: {message}")]
    Parse { path: PathBuf, message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scope {
    /// Hooks live in the repository (`.claude/settings.json`, `.git/hooks`).
    Project,
    /// Hooks live in the user's home directory and apply to every project.
    User,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Project => "project",
            Scope::User => "user",
        }
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OnFailure {
    /// Send the failure back to the agent until the retry budget is used up, then ask.
    RetryThenAsk,
    /// Stop and ask the user on the first failure.
    Ask,
    /// Let the agent stop and only show the user a message.
    Notify,
}

impl OnFailure {
    pub fn as_str(self) -> &'static str {
        match self {
            OnFailure::RetryThenAsk => "retry-then-ask",
            OnFailure::Ask => "ask",
            OnFailure::Notify => "notify",
        }
    }
}

impl fmt::Display for OnFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for OnFailure {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "retry-then-ask" => Ok(OnFailure::RetryThenAsk),
            "ask" => Ok(OnFailure::Ask),
            "notify" => Ok(OnFailure::Notify),
            other => Err(format!(
                "unknown value `{other}`, expected one of `retry-then-ask`, `ask`, `notify`"
            )),
        }
    }
}

impl std::str::FromStr for Scope {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "project" => Ok(Scope::Project),
            "user" => Ok(Scope::User),
            other => Err(format!(
                "unknown value `{other}`, expected `project` or `user`"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentsConfig {
    pub enabled: Vec<String>,
    pub scope: Scope,
}

impl Default for AgentsConfig {
    fn default() -> Self {
        Self {
            enabled: vec!["claude-code".into(), "git".into()],
            scope: Scope::Project,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProtectConfig {
    pub paths: Vec<String>,
}

impl Default for ProtectConfig {
    fn default() -> Self {
        Self {
            paths: DEFAULT_PROTECTED_PATHS
                .iter()
                .map(|p| p.to_string())
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TestsConfig {
    /// Shell command run at the end of an agent turn. Empty means "do not run tests".
    pub command: String,
    pub on_failure: OnFailure,
    pub max_retries: u32,
}

impl Default for TestsConfig {
    fn default() -> Self {
        Self {
            command: "cargo test".into(),
            on_failure: OnFailure::RetryThenAsk,
            max_retries: 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReviewConfig {
    /// Changed lines (added + removed) above which the user gets a "please review" note.
    pub max_diff_lines: usize,
}

impl Default for ReviewConfig {
    fn default() -> Self {
        Self {
            max_diff_lines: 800,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GitConfig {
    /// Run `tests.command` in the git pre-commit hook.
    pub run_tests: bool,
}

impl Default for GitConfig {
    fn default() -> Self {
        Self { run_tests: true }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub agents: AgentsConfig,
    pub protect: ProtectConfig,
    pub tests: TestsConfig,
    pub review: ReviewConfig,
    pub git: GitConfig,
}

/// Where a loaded [`Config`] came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    Project(PathBuf),
    User(PathBuf),
    Defaults,
}

impl ConfigSource {
    pub fn describe(&self) -> String {
        match self {
            ConfigSource::Project(p) => format!("project config {}", p.display()),
            ConfigSource::User(p) => format!("user config {}", p.display()),
            ConfigSource::Defaults => "built-in defaults (no config file found)".to_string(),
        }
    }
}

impl Config {
    /// Parses config text. `origin` is only used in error messages.
    pub fn parse(text: &str, origin: &Path) -> Result<Config, ConfigError> {
        toml::from_str(text).map_err(|e| ConfigError::Parse {
            path: origin.to_path_buf(),
            message: e.message().to_string(),
        })
    }

    pub fn load_file(path: &Path) -> Result<Config, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Config::parse(&text, path)
    }

    /// Path of the per-user config file.
    pub fn user_config_path() -> Option<PathBuf> {
        dirs::home_dir().map(|h| user_config_path_in(&h))
    }

    /// Finds and loads the config that applies to `start`.
    pub fn discover(start: &Path) -> Result<(Config, ConfigSource), ConfigError> {
        Config::discover_with(start, Config::user_config_path().as_deref())
    }

    /// Like [`Config::discover`] with an explicit user config path (used by tests).
    pub fn discover_with(
        start: &Path,
        user_config: Option<&Path>,
    ) -> Result<(Config, ConfigSource), ConfigError> {
        if let Some(path) = find_project_config(start) {
            let cfg = Config::load_file(&path)?;
            return Ok((cfg, ConfigSource::Project(path)));
        }
        if let Some(path) = user_config {
            if path.is_file() {
                let cfg = Config::load_file(path)?;
                return Ok((cfg, ConfigSource::User(path.to_path_buf())));
            }
        }
        Ok((Config::default(), ConfigSource::Defaults))
    }
}

pub fn user_config_path_in(home: &Path) -> PathBuf {
    home.join(".config").join("reflex").join("reflex.toml")
}

/// Walks up from `start` looking for `.reflex.toml`. The search stops after the
/// directory that contains `.git`, so a config in an unrelated parent is not picked up
/// from inside another repository.
pub fn find_project_config(start: &Path) -> Option<PathBuf> {
    for dir in start.ancestors() {
        let candidate = dir.join(PROJECT_CONFIG_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        if dir.join(".git").exists() {
            return None;
        }
    }
    None
}

/// Nearest ancestor of `start` (inclusive) that contains `.git`.
pub fn find_repo_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|d| d.join(".git").exists())
        .map(Path::to_path_buf)
}

fn toml_str(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

fn toml_list(items: &[String]) -> String {
    let inner: Vec<String> = items.iter().map(|s| toml_str(s)).collect();
    format!("[{}]", inner.join(", "))
}

/// Renders a config as commented TOML, the format the installer writes.
pub fn render(cfg: &Config) -> String {
    let paths = if cfg.protect.paths.len() > 3 {
        let lines: Vec<String> = cfg
            .protect
            .paths
            .iter()
            .map(|p| format!("    {},", toml_str(p)))
            .collect();
        format!("[\n{}\n]", lines.join("\n"))
    } else {
        toml_list(&cfg.protect.paths)
    };
    format!(
        "# Reflex Control settings. Edit and re-run `reflex install` to apply agent changes.\n\
         \n\
         [agents]\n\
         enabled = {enabled}\n\
         scope = {scope} # \"project\" | \"user\"\n\
         \n\
         [protect]\n\
         # Agents may not write to these paths. Patterns without a `/` match the file\n\
         # name at any depth; patterns with a `/` are relative to the project root.\n\
         paths = {paths}\n\
         \n\
         [tests]\n\
         command = {command} # empty string = do not run tests\n\
         on_failure = {on_failure} # \"retry-then-ask\" | \"ask\" | \"notify\"\n\
         max_retries = {max_retries}\n\
         \n\
         [review]\n\
         max_diff_lines = {max_diff}\n\
         \n\
         [git]\n\
         run_tests = {run_tests}\n",
        enabled = toml_list(&cfg.agents.enabled),
        scope = toml_str(cfg.agents.scope.as_str()),
        paths = paths,
        command = toml_str(&cfg.tests.command),
        on_failure = toml_str(cfg.tests.on_failure.as_str()),
        max_retries = cfg.tests.max_retries,
        max_diff = cfg.review.max_diff_lines,
        run_tests = cfg.git.run_tests,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin() -> PathBuf {
        PathBuf::from(".reflex.toml")
    }

    #[test]
    fn empty_file_gives_defaults() {
        let cfg = Config::parse("", &origin()).unwrap();
        assert_eq!(cfg, Config::default());
        assert_eq!(cfg.agents.enabled, ["claude-code", "git"]);
        assert_eq!(cfg.agents.scope, Scope::Project);
        assert_eq!(cfg.tests.command, "cargo test");
        assert_eq!(cfg.tests.on_failure, OnFailure::RetryThenAsk);
        assert_eq!(cfg.tests.max_retries, 2);
        assert_eq!(cfg.review.max_diff_lines, 800);
        assert!(cfg.git.run_tests);
        assert!(cfg.protect.paths.contains(&"secrets/**".to_string()));
    }

    #[test]
    fn partial_file_overrides_only_what_it_sets() {
        let cfg = Config::parse(
            "[tests]\ncommand = \"npm test\"\non_failure = \"notify\"\n\n[protect]\npaths = [\"a.txt\"]\n",
            &origin(),
        )
        .unwrap();
        assert_eq!(cfg.tests.command, "npm test");
        assert_eq!(cfg.tests.on_failure, OnFailure::Notify);
        assert_eq!(cfg.tests.max_retries, 2);
        assert_eq!(cfg.protect.paths, ["a.txt"]);
        assert_eq!(cfg.review, ReviewConfig::default());
    }

    #[test]
    fn invalid_value_names_the_file_and_the_choices() {
        let err = Config::parse("[tests]\non_failure = \"explode\"\n", &origin()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains(".reflex.toml"), "{msg}");
        assert!(msg.contains("explode"), "{msg}");
        assert!(msg.contains("retry-then-ask"), "{msg}");
    }

    #[test]
    fn wrong_type_is_an_error() {
        let err = Config::parse("[tests]\nmax_retries = \"two\"\n", &origin()).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
    }

    #[test]
    fn render_round_trips() {
        let mut cfg = Config::default();
        cfg.tests.command = "make \"check\"".into();
        cfg.tests.on_failure = OnFailure::Ask;
        cfg.agents.scope = Scope::User;
        cfg.protect.paths.push("weird\\path".into());
        let text = render(&cfg);
        assert_eq!(Config::parse(&text, &origin()).unwrap(), cfg);
        // The default config renders to something that parses back to the defaults.
        assert_eq!(
            Config::parse(&render(&Config::default()), &origin()).unwrap(),
            Config::default()
        );
    }

    #[test]
    fn discover_prefers_project_then_user_then_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let sub = repo.join("src/deep");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let user = tmp.path().join("user.toml");
        std::fs::write(&user, "[tests]\ncommand = \"user-cmd\"\n").unwrap();

        // No project file: user config wins over defaults.
        let (cfg, src) = Config::discover_with(&sub, Some(&user)).unwrap();
        assert_eq!(cfg.tests.command, "user-cmd");
        assert_eq!(src, ConfigSource::User(user.clone()));

        // A project file in a parent directory is found from a subdirectory.
        std::fs::write(repo.join(".reflex.toml"), "[tests]\ncommand = \"proj\"\n").unwrap();
        let (cfg, src) = Config::discover_with(&sub, Some(&user)).unwrap();
        assert_eq!(cfg.tests.command, "proj");
        assert_eq!(src, ConfigSource::Project(repo.join(".reflex.toml")));
    }

    #[test]
    fn search_stops_at_the_repo_root() {
        let tmp = tempfile::tempdir().unwrap();
        // A config above the repo must not apply to the repo.
        std::fs::write(
            tmp.path().join(".reflex.toml"),
            "[tests]\ncommand = \"outer\"\n",
        )
        .unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let (cfg, src) = Config::discover_with(&repo, None).unwrap();
        assert_eq!(cfg, Config::default());
        assert_eq!(src, ConfigSource::Defaults);
    }
}
