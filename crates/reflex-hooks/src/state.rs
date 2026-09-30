//! Per-session state kept between hook calls, in `.reflex/state/<session>.json`.
//!
//! It holds the retry counter for the session and a fingerprint of the working tree at
//! the last turn-end check, so an unchanged tree is not tested twice. The directory
//! gets its own `.gitignore` (`*`), so it never shows up in `git status`, even where the
//! project's `.gitignore` was not edited (user-scope installs).

use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// State files older than this are removed when a new one is written.
const MAX_STATE_AGE: Duration = Duration::from_secs(14 * 24 * 3600);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionState {
    /// Times this session was sent back to fix failing tests since the last pass.
    pub retries: u32,
    /// Fingerprint of the working tree at the last check.
    pub fingerprint: Option<String>,
    /// True if that check did not fail.
    pub last_ok: bool,
}

/// An untracked file, as far as the fingerprint cares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UntrackedFile {
    pub path: String,
    pub len: u64,
    pub mtime_nanos: u128,
}

/// Makes a session id safe to use as a file name.
pub fn sanitize_session(id: &str) -> String {
    let cleaned: String = id
        .chars()
        .take(64)
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "default".to_string()
    } else {
        cleaned
    }
}

pub fn state_dir(root: &Path) -> PathBuf {
    root.join(".reflex").join("state")
}

pub fn state_path(root: &Path, session: &str) -> PathBuf {
    state_dir(root).join(format!("{}.json", sanitize_session(session)))
}

/// Loads the session's state; a missing or unreadable file gives the empty state.
pub fn load(root: &Path, session: &str) -> SessionState {
    std::fs::read_to_string(state_path(root, session))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(root: &Path, session: &str, state: &SessionState) -> io::Result<()> {
    let dir = state_dir(root);
    std::fs::create_dir_all(&dir)?;
    let ignore = root.join(".reflex").join(".gitignore");
    if !ignore.exists() {
        std::fs::write(&ignore, "*\n")?;
    }
    let text = serde_json::to_string(state).expect("state always serializes");
    std::fs::write(state_path(root, session), text)?;
    prune(&dir);
    Ok(())
}

fn prune(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|age| age > MAX_STATE_AGE);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// FNV-1a, 64 bit. Not cryptographic; it only has to notice that the tree changed, and
/// unlike `DefaultHasher` its output is the same across Rust versions.
struct Fnv(u64);

impl Fnv {
    fn new() -> Fnv {
        Fnv(0xcbf2_9ce4_8422_2325)
    }

    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= u64::from(*b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
        // Separator, so ("ab", "c") and ("a", "bc") differ.
        self.0 ^= 0xff;
        self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// Fingerprint of the working tree: the `git diff HEAD` text plus the untracked files.
/// Untracked files count by name, size and modification time, so editing a new file
/// changes the fingerprint even though the list of names does not.
pub fn fingerprint(diff: &str, untracked: &[UntrackedFile]) -> String {
    let mut h = Fnv::new();
    h.write(diff.as_bytes());
    let mut sorted: Vec<&UntrackedFile> = untracked.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    for f in sorted {
        h.write(f.path.as_bytes());
        h.write(&f.len.to_le_bytes());
        h.write(&f.mtime_nanos.to_le_bytes());
    }
    format!("{:016x}", h.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_ids_become_safe_file_names() {
        assert_eq!(sanitize_session("abc-123_X"), "abc-123_X");
        assert_eq!(sanitize_session("../../etc/passwd"), "______etc_passwd");
        assert_eq!(sanitize_session(""), "default");
        assert_eq!(sanitize_session(&"a".repeat(200)).len(), 64);
    }

    #[test]
    fn state_round_trips_and_ignores_itself() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(load(tmp.path(), "s1"), SessionState::default());
        let st = SessionState {
            retries: 2,
            fingerprint: Some("abc".into()),
            last_ok: true,
        };
        save(tmp.path(), "s1", &st).unwrap();
        assert_eq!(load(tmp.path(), "s1"), st);
        assert_eq!(load(tmp.path(), "other"), SessionState::default());
        let ignore = std::fs::read_to_string(tmp.path().join(".reflex/.gitignore")).unwrap();
        assert_eq!(ignore, "*\n");
    }

    #[test]
    fn corrupt_state_file_is_treated_as_empty() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(state_dir(tmp.path())).unwrap();
        std::fs::write(state_path(tmp.path(), "s"), "{ nope").unwrap();
        assert_eq!(load(tmp.path(), "s"), SessionState::default());
    }

    #[test]
    fn fingerprint_changes_with_diff_and_untracked_files() {
        let f = |path: &str, len: u64, mtime: u128| UntrackedFile {
            path: path.into(),
            len,
            mtime_nanos: mtime,
        };
        let base = fingerprint("diff a", &[f("x", 1, 10)]);
        assert_eq!(base, fingerprint("diff a", &[f("x", 1, 10)]));
        assert_ne!(base, fingerprint("diff b", &[f("x", 1, 10)]));
        assert_ne!(base, fingerprint("diff a", &[f("x", 2, 10)]));
        assert_ne!(base, fingerprint("diff a", &[f("x", 1, 11)]));
        assert_ne!(base, fingerprint("diff a", &[f("y", 1, 10)]));
        assert_ne!(base, fingerprint("diff a", &[]));
        // Order of the untracked list does not matter.
        assert_eq!(
            fingerprint("d", &[f("a", 1, 1), f("b", 2, 2)]),
            fingerprint("d", &[f("b", 2, 2), f("a", 1, 1)])
        );
    }
}
