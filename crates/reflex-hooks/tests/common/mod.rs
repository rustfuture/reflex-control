//! Shared by the golden tests of the agents that have no test file of their own logic:
//! recorded hook input in, exact stdout and exit code out. Each case is a set of files
//! in `tests/fixtures/<dir>/`:
//!
//! * `<event>__<case>.in.json`: what the agent (or its plugin) writes to the hook's stdin
//! * `<event>__<case>.out`: the exact stdout expected
//! * `<event>__<case>.stderr` (optional): text that stderr must start with
//! * `<event>__<case>.verdict` (turn-end cases): the turn-end verdict to feed in, as
//!   `<kind>: <text>`, since the real turn-end check needs git and a test suite

use reflex_hooks::config::{Config, ConfigSource};
use reflex_hooks::guard::{TestOutcome, Verdict};
use reflex_hooks::runtime::{handle, Project, Runtime};
use std::io;
use std::path::{Path, PathBuf};

struct Fixture {
    stdin: String,
    verdict: Verdict,
}

impl Runtime for Fixture {
    fn read_stdin(&self) -> io::Result<String> {
        Ok(self.stdin.clone())
    }

    fn current_dir(&self) -> PathBuf {
        PathBuf::from("/work/proj")
    }

    fn project(&self, _cwd: &Path) -> Result<Project, String> {
        Ok(Project {
            root: PathBuf::from("/work/proj"),
            config: Config::default(),
            source: ConfigSource::Defaults,
        })
    }

    fn turn_end(&self, _project: &Project, _session_id: &str) -> Result<Verdict, String> {
        Ok(self.verdict.clone())
    }

    fn staged_files(&self, _project: &Project) -> Result<Vec<String>, String> {
        Ok(Vec::new())
    }

    fn pre_commit_tests(&self, _project: &Project) -> Result<TestOutcome, String> {
        Ok(TestOutcome::Passed)
    }
}

fn parse_verdict(text: &str) -> Verdict {
    let text = text.trim_end_matches('\n');
    let (kind, rest) = text.split_once(':').unwrap_or((text, ""));
    // Newlines are written as `\n` in the fixture so it stays one line.
    let rest = rest.trim_start().replace("\\n", "\n");
    match kind {
        "allow" => Verdict::Allow,
        "retry" => Verdict::RetryAgent { reason: rest },
        "ask-human" => Verdict::AskHuman { reason: rest },
        "notify" => Verdict::Notify { message: rest },
        other => panic!("unknown verdict kind {other:?}"),
    }
}

/// Runs every case in `tests/fixtures/<dir>/` through `reflex hook <agent> <event>`.
pub fn run_fixtures(agent: &str, dir: &str, min_cases: usize) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(dir);
    let mut cases: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(".in.json"))
        .collect();
    cases.sort();
    assert!(
        cases.len() >= min_cases,
        "fixtures went missing: {}",
        cases.len()
    );

    for input in cases {
        let file = input.file_name().unwrap().to_string_lossy().into_owned();
        let stem = file.trim_end_matches(".in.json");
        let (event, _) = stem
            .split_once("__")
            .expect("fixture name is <event>__<case>");
        let sibling = |ext: &str| dir.join(format!("{stem}.{ext}"));

        let verdict = std::fs::read_to_string(sibling("verdict"))
            .map(|t| parse_verdict(&t))
            .unwrap_or(Verdict::Allow);
        let rt = Fixture {
            stdin: std::fs::read_to_string(&input).unwrap(),
            verdict,
        };

        let got = handle(agent, event, &rt);

        let expected = std::fs::read_to_string(sibling("out")).unwrap();
        assert_eq!(got.stdout, expected, "stdout of {stem}");
        assert_eq!(got.exit_code, 0, "exit code of {stem}");
        match std::fs::read_to_string(sibling("stderr")) {
            Ok(prefix) => assert!(
                got.stderr.starts_with(prefix.trim_end()),
                "stderr of {stem}: {:?}",
                got.stderr
            ),
            Err(_) => assert_eq!(got.stderr, "", "stderr of {stem}"),
        }
    }
}
