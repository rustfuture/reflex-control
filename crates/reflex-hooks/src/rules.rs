//! Destructive commands that need a person's yes before an agent runs them.
//!
//! Every rule has a stable id, `<category>.<name>`, which the ask message names and
//! `commands.allow` in `.reflex.toml` can switch off. The rules look at one simple
//! command at a time (words after wrappers such as `sudo` or `npx`), plus any text the
//! command reads on stdin (heredocs, here-strings, `echo`/`printf`/`cat <<EOF` piped into
//! it), so SQL piped into `psql` counts. For `docker|podman exec` and `kubectl|oc exec --`
//! the command run inside the container is checked too.
//!
//! Like the rest of the shell check this is a heuristic over the command text. It aims
//! at the commands that lose data or history in one step and are hard to undo; it does
//! not try to judge every command that changes something.

use crate::guard::{has_short_flag, is_broad_target, is_drive_path, is_flag, option_values};

/// A rule that matched: its id and why the command needs confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub rule: &'static str,
    pub why: String,
}

/// Every rule id with a one-line description, for `reflex check --rules` and the docs.
pub const RULES: &[(&str, &str)] = &[
    (
        "fs.rm-rf-broad",
        "rm -r on /, ~, ., *, a top-level directory, or --no-preserve-root",
    ),
    (
        "fs.find-delete",
        "find -delete or -exec rm over /, ~, . or * without a name filter",
    ),
    (
        "fs.chmod-broad",
        "recursive chmod, chown or chgrp over /, ~ or a top-level directory",
    ),
    (
        "fs.disk",
        "mkfs, wipefs, partitioning tools, diskutil erase, dd onto a device",
    ),
    (
        "git.push-force",
        "git push --force, -f or +refspec (--force-with-lease is allowed)",
    ),
    (
        "git.commit-no-verify",
        "git commit --no-verify or -n, which skips the pre-commit hook",
    ),
    ("git.reset-hard", "git reset --hard"),
    (
        "git.clean-force",
        "git clean -f (dry runs with -n are allowed)",
    ),
    (
        "git.discard-changes",
        "git checkout/restore of the whole tree, git checkout -f",
    ),
    (
        "git.branch-force-delete",
        "git branch -D or --delete --force",
    ),
    ("git.stash-drop", "git stash drop or clear"),
    ("git.push-delete", "git push --delete, -d or :branch"),
    (
        "git.history-rewrite",
        "git filter-branch, filter-repo, reflog expire, update-ref -d",
    ),
    (
        "db.sql",
        "DROP, TRUNCATE, DROP COLUMN or DELETE without WHERE sent to a database client",
    ),
    ("db.drop", "dropdb, mysqladmin drop"),
    ("db.redis-flush", "redis-cli FLUSHALL or FLUSHDB"),
    (
        "db.orm-reset",
        "prisma migrate reset, rails db:drop, manage.py flush and similar",
    ),
    (
        "container.prune",
        "docker/podman system or volume prune, volume rm, compose down -v",
    ),
    (
        "k8s.delete",
        "kubectl delete of namespaces, volumes or --all, kubectl drain, helm uninstall",
    ),
    (
        "iac.destroy",
        "terraform/tofu/pulumi/cdk destroy, apply -auto-approve, state rm",
    ),
    (
        "cloud.delete",
        "delete/destroy commands of cloud and hosting CLIs (aws, gcloud, az, gh, ...)",
    ),
    (
        "http.delete",
        "HTTP DELETE requests and GraphQL delete mutations from curl, wget, httpie",
    ),
];

fn hit(rule: &'static str, why: &str) -> Hit {
    Hit {
        rule,
        why: format!("this command {why}"),
    }
}

/// Rules that match `cmd args...` (`cmd` is the program's base name).
pub fn check(cmd: &str, args: &[String], stdin_texts: &[String]) -> Vec<Hit> {
    let ops: Vec<&str> = args
        .iter()
        .filter(|a| !is_flag(a))
        .map(String::as_str)
        .collect();
    let has = |names: &[&str]| args.iter().any(|a| names.contains(&a.as_str()));
    let mut hits = Vec::new();

    match cmd {
        "rm" => {
            let recursive = args
                .iter()
                .any(|a| a == "--recursive" || has_short_flag(a, &['r', 'R']));
            if recursive && (has(&["--no-preserve-root"]) || ops.iter().any(|t| is_broad_target(t)))
            {
                hits.push(hit("fs.rm-rf-broad", "recursively deletes a broad target"));
            }
        }
        "find" if find_deletes_broadly(args) => {
            hits.push(hit(
                "fs.find-delete",
                "deletes every match of a find over a broad target",
            ));
        }
        "chmod" | "chown" | "chgrp" => {
            let recursive = args
                .iter()
                .any(|a| a == "--recursive" || has_short_flag(a, &['R']));
            // The project directory itself (`.`) is a normal target for a permission fix.
            let broad = ops
                .iter()
                .any(|t| is_broad_target(t) && !matches!(*t, "." | "./" | "*" | "./*"));
            if recursive && broad {
                hits.push(hit(
                    "fs.chmod-broad",
                    "changes permissions recursively on a broad target",
                ));
            }
        }
        c if c.starts_with("mkfs") || c == "wipefs" => {
            hits.push(hit("fs.disk", "erases a disk or partition"));
        }
        "fdisk" | "sfdisk" | "gdisk"
            if ops.iter().any(|o| o.starts_with("/dev/"))
                && !args
                    .iter()
                    .any(|a| matches!(a.as_str(), "-l" | "--list" | "-s" | "--show-size")) =>
        {
            hits.push(hit("fs.disk", "repartitions a disk"));
        }
        "parted"
            if ops.iter().any(|o| {
                matches!(
                    *o,
                    "mklabel" | "mktable" | "mkpart" | "rm" | "resizepart" | "rescue"
                )
            }) =>
        {
            hits.push(hit("fs.disk", "repartitions a disk"));
        }
        "diskutil"
            if ops.first().is_some_and(|o| {
                let o = o.to_ascii_lowercase();
                o.starts_with("erase")
                    || [
                        "partitiondisk",
                        "zerodisk",
                        "randomdisk",
                        "secureerase",
                        "reformat",
                    ]
                    .contains(&o.as_str())
            }) =>
        {
            hits.push(hit("fs.disk", "erases a disk or volume"));
        }
        "dd" if args
            .iter()
            .any(|a| a.strip_prefix("of=").is_some_and(is_device)) =>
        {
            hits.push(hit("fs.disk", "writes raw data onto a device"));
        }
        "git" => git_rules(args, &mut hits),
        _ => {}
    }
    windows_rules(cmd, args, &mut hits);

    if let Some((inner, rest)) = exec_command(cmd, args) {
        hits.extend(check(crate::guard::base_name(inner), rest, stdin_texts));
    }
    db_rules(cmd, args, &ops, stdin_texts, &mut hits);
    infra_rules(cmd, args, &ops, &mut hits);
    http_rules(cmd, args, &ops, &mut hits);
    hits
}

/// cmd.exe and PowerShell commands, whose names are case-insensitive.
fn windows_rules(cmd: &str, args: &[String], hits: &mut Vec<Hit>) {
    // `/s`-style switches would otherwise look like top-level Unix paths.
    let is_switch = |a: &str| {
        a.len() <= 3 && a.starts_with('/') && a[1..].chars().all(|c| c.is_ascii_alphabetic())
    };
    let switch = |c: char| {
        args.iter()
            .any(|a| a.eq_ignore_ascii_case(&format!("/{c}")))
    };
    let targets: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|a| !is_switch(a) && !is_flag(a))
        .collect();
    let broad = targets.iter().any(|t| is_broad_target(t));
    let recursive_ps = args.iter().any(|a| a.eq_ignore_ascii_case("-recurse"));
    match cmd.to_ascii_lowercase().as_str() {
        "rmdir" | "rd" | "del" | "erase" if switch('s') && broad => {
            hits.push(hit("fs.rm-rf-broad", "recursively deletes a broad target"));
        }
        "remove-item" | "ri" if recursive_ps && broad => {
            hits.push(hit("fs.rm-rf-broad", "recursively deletes a broad target"));
        }
        "format" if targets.iter().any(|t| is_drive_path(t) && t.len() == 2) => {
            hits.push(hit("fs.disk", "erases a disk or partition"));
        }
        _ => {}
    }
}

/// The command that `docker exec`, `docker compose exec` or `kubectl exec ... --` runs.
fn exec_command<'a>(cmd: &str, args: &'a [String]) -> Option<(&'a str, &'a [String])> {
    let exec_at = args.iter().position(|a| a == "exec")?;
    match cmd {
        "kubectl" | "oc" => {
            let dash = args.iter().position(|a| a == "--")?;
            let (inner, rest) = args.get(dash + 1..)?.split_first()?;
            Some((inner.as_str(), rest))
        }
        "docker" | "podman" | "nerdctl" | "docker-compose" | "podman-compose" => {
            // Only `docker exec` / `docker compose exec` / `docker container exec`.
            let before_ok = args[..exec_at]
                .iter()
                .filter(|a| !is_flag(a))
                .all(|w| w == "compose" || w == "container");
            if !before_ok {
                return None;
            }
            let takes_value = [
                "-e",
                "--env",
                "-u",
                "--user",
                "-w",
                "--workdir",
                "--env-file",
                "--index",
            ];
            let mut i = exec_at + 1;
            while let Some(a) = args.get(i) {
                if !is_flag(a) {
                    break;
                }
                i += if takes_value.contains(&a.as_str()) {
                    2
                } else {
                    1
                };
            }
            // args[i] is the container or service; the command follows it.
            let (inner, rest) = args.get(i + 1..)?.split_first()?;
            Some((inner.as_str(), rest))
        }
        _ => None,
    }
}

/// Device paths whose overwrite loses data (`/dev/null` and friends are fine).
fn is_device(path: &str) -> bool {
    path.strip_prefix("/dev/").is_some_and(|d| {
        !matches!(
            d,
            "null" | "zero" | "stdout" | "stderr" | "tty" | "random" | "urandom"
        ) && !d.starts_with("fd/")
    })
}

fn find_deletes_broadly(args: &[String]) -> bool {
    let starts: Vec<&String> = args
        .iter()
        .take_while(|a| !a.starts_with('-') && !a.starts_with('(') && !a.starts_with('!'))
        .collect();
    let broad = starts.is_empty() || starts.iter().any(|s| is_broad_target(s));
    // `find . -name '*.pyc' -delete` only removes what the name filter selects.
    let filtered = args.windows(2).any(|w| {
        matches!(
            w[0].as_str(),
            "-name" | "-iname" | "-path" | "-ipath" | "-wholename" | "-regex" | "-iregex"
        ) && !matches!(w[1].as_str(), "*" | "*.*")
    });
    if filtered {
        return false;
    }
    let deletes = args.iter().enumerate().any(|(i, a)| {
        a == "-delete"
            || (matches!(a.as_str(), "-exec" | "-execdir" | "-ok" | "-okdir")
                && args.get(i + 1).is_some_and(|c| {
                    matches!(
                        crate::guard::base_name(c),
                        "rm" | "shred" | "unlink" | "rmdir"
                    )
                }))
    });
    broad && deletes
}

/// Splits `git` arguments into the subcommand and its arguments, skipping git's own
/// options (`-C dir`, `-c key=val`, ...).
pub(crate) fn git_subcommand(args: &[String]) -> Option<(&str, &[String])> {
    let mut i = 0;
    while let Some(a) = args.get(i) {
        if a == "-C" || a == "-c" || a == "--git-dir" || a == "--work-tree" {
            i += 2;
        } else if is_flag(a) {
            i += 1;
        } else {
            return Some((a.as_str(), &args[i + 1..]));
        }
    }
    None
}

/// Pathspecs that mean "the whole working tree".
fn is_whole_tree(spec: &str) -> bool {
    matches!(spec, "." | "./" | "*" | ":/" | ":(top)" | ":/*")
}

fn git_rules(args: &[String], hits: &mut Vec<Hit>) {
    let Some((sub, rest)) = git_subcommand(args) else {
        return;
    };
    let has = |names: &[&str]| rest.iter().any(|a| names.contains(&a.as_str()));
    let short = |c: char| rest.iter().any(|a| has_short_flag(a, &[c]));
    let ops: Vec<&str> = rest
        .iter()
        .filter(|a| !is_flag(a))
        .map(String::as_str)
        .collect();

    match sub {
        "push" => {
            let forced = rest.iter().any(|a| {
                a == "--force" || short_only(a, 'f') || (a.starts_with('+') && a.len() > 1)
            });
            if forced {
                hits.push(hit(
                    "git.push-force",
                    "force-pushes and can overwrite remote history",
                ));
            }
            if has(&["--delete"])
                || short_only_any(rest, 'd')
                || ops.iter().any(|o| o.len() > 1 && o.starts_with(':'))
            {
                hits.push(hit(
                    "git.push-delete",
                    "deletes a branch or tag on the remote",
                ));
            }
        }
        "commit" if has(&["--no-verify"]) || rest.iter().any(|a| short_only(a, 'n')) => {
            hits.push(hit(
                "git.commit-no-verify",
                "commits with --no-verify and skips the pre-commit checks",
            ));
        }
        "reset" if has(&["--hard"]) => {
            hits.push(hit(
                "git.reset-hard",
                "throws away uncommitted changes (git reset --hard)",
            ));
        }
        "clean" if (has(&["--force"]) || short('f')) && !(has(&["--dry-run"]) || short('n')) => {
            hits.push(hit(
                "git.clean-force",
                "deletes untracked files (git clean -f)",
            ));
        }
        "checkout" | "restore" => {
            let paths: Vec<&str> = match rest.iter().position(|a| a == "--") {
                Some(dash) => rest[dash + 1..].iter().map(String::as_str).collect(),
                None => ops.clone(),
            };
            let worktree =
                sub == "checkout" || has(&["--worktree", "-W"]) || !(has(&["--staged", "-S"]));
            let forced = sub == "checkout" && (has(&["--force"]) || short('f'));
            if forced || (worktree && paths.iter().any(|p| is_whole_tree(p))) {
                hits.push(hit(
                    "git.discard-changes",
                    "throws away uncommitted changes in the working tree",
                ));
            }
        }
        "branch" => {
            let force_delete = short('D')
                || ((has(&["--delete"]) || short('d')) && (has(&["--force"]) || short('f')));
            if force_delete {
                hits.push(hit(
                    "git.branch-force-delete",
                    "deletes a branch even if it is not merged",
                ));
            }
        }
        "stash" if matches!(ops.first(), Some(&"drop") | Some(&"clear")) => {
            hits.push(hit("git.stash-drop", "deletes stashed changes"));
        }
        "filter-branch" | "filter-repo" => {
            hits.push(hit(
                "git.history-rewrite",
                "rewrites the repository history",
            ));
        }
        "reflog" if ops.first() == Some(&"expire") => {
            hits.push(hit(
                "git.history-rewrite",
                "expires reflog entries, which are the undo history",
            ));
        }
        "update-ref" if has(&["-d"]) => {
            hits.push(hit("git.history-rewrite", "deletes a ref directly"));
        }
        _ => {}
    }
}

/// `-f` style flag: a short-option cluster containing `letter` (not a long option).
fn short_only(arg: &str, letter: char) -> bool {
    has_short_flag(arg, &[letter])
}

fn short_only_any(args: &[String], letter: char) -> bool {
    args.iter().any(|a| short_only(a, letter))
}

// ─────────────────────────────────────────────────────────────────────────────
// Databases
// ─────────────────────────────────────────────────────────────────────────────

/// Clients whose arguments and stdin carry SQL (or Mongo shell code), with the options
/// that take a statement.
fn sql_options(cmd: &str) -> Option<(&'static [char], &'static [&'static str])> {
    Some(match cmd {
        "psql" | "usql" => (&['c'], &["--command"]),
        "mysql" | "mariadb" => (&['e'], &["--execute"]),
        "sqlite3" | "sqlite" | "duckdb" => (&['c'], &["-cmd"]),
        "clickhouse-client" | "snowsql" => (&['q'], &["--query"]),
        "cockroach" => (&['e'], &["--execute"]),
        "mongosh" | "mongo" => (&[], &["--eval"]),
        _ => return None,
    })
}

fn db_rules(cmd: &str, args: &[String], ops: &[&str], stdin: &[String], hits: &mut Vec<Hit>) {
    if let Some((short, long)) = sql_options(cmd) {
        let mut texts = option_values(args, short, long);
        if matches!(cmd, "sqlite3" | "sqlite" | "duckdb") {
            // `sqlite3 app.db "SQL"`: everything after the database file is SQL.
            texts.extend(ops.iter().skip(1).map(|s| s.to_string()));
        }
        texts.extend(stdin.iter().cloned());
        if texts.iter().any(|t| sql_is_destructive(t)) {
            hits.push(hit(
                "db.sql",
                "sends a statement that deletes data or tables",
            ));
        }
    }

    let lower: Vec<String> = ops.iter().map(|o| o.to_ascii_lowercase()).collect();
    let word = |w: &str| lower.iter().any(|o| o == w);
    match cmd {
        "dropdb" => hits.push(hit("db.drop", "drops a database")),
        "mysqladmin" if word("drop") => hits.push(hit("db.drop", "drops a database")),
        "redis-cli" | "valkey-cli" | "keydb-cli"
            if word("flushall")
                || word("flushdb")
                || stdin.iter().any(|t| {
                    let t = t.to_ascii_lowercase();
                    t.contains("flushall") || t.contains("flushdb")
                }) =>
        {
            hits.push(hit("db.redis-flush", "deletes every key in the database"));
        }
        _ => {}
    }

    let resets = match cmd {
        "prisma" => {
            (ops.first() == Some(&"migrate") && ops.get(1) == Some(&"reset"))
                || (ops.first() == Some(&"db")
                    && args
                        .iter()
                        .any(|a| a == "--force-reset" || a == "--accept-data-loss"))
        }
        "rails" | "rake" => ops
            .iter()
            .any(|o| matches!(*o, "db:drop" | "db:reset" | "db:purge" | "db:truncate_all")),
        c if crate::guard::is_python(c) => {
            ops.first().is_some_and(|f| f.ends_with("manage.py"))
                && matches!(ops.get(1), Some(&"flush") | Some(&"reset_db"))
        }
        "django-admin" => matches!(ops.first(), Some(&"flush") | Some(&"reset_db")),
        "supabase" => ops.first() == Some(&"db") && ops.get(1) == Some(&"reset"),
        "sequelize" | "sequelize-cli" => ops.first() == Some(&"db:drop"),
        "typeorm" => ops.first() == Some(&"schema:drop"),
        "mix" => ops.iter().any(|o| matches!(*o, "ecto.drop" | "ecto.reset")),
        c if c.starts_with("php") => {
            ops.first() == Some(&"artisan")
                && ops
                    .get(1)
                    .is_some_and(|o| matches!(*o, "migrate:fresh" | "migrate:reset" | "db:wipe"))
        }
        _ => false,
    };
    if resets {
        hits.push(hit(
            "db.orm-reset",
            "drops or resets the application database",
        ));
    }
}

/// True if `text` has a statement that drops or empties a table, schema or database,
/// a `DELETE` without `WHERE`, or a Mongo call that does the same.
pub fn sql_is_destructive(text: &str) -> bool {
    let upper = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_uppercase();
    let sql = upper.split(';').any(|stmt| {
        let s = stmt.trim();
        s.contains("DROP DATABASE")
            || s.contains("DROP SCHEMA")
            || s.contains("DROP TABLE")
            || s.contains("DROP COLUMN")
            || s.starts_with("TRUNCATE")
            || s.contains(" TRUNCATE ")
            || (s.contains("DELETE FROM") && !s.contains(" WHERE "))
    });
    let compact: String = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    let mongo = compact.contains(".dropdatabase(")
        || compact.contains(".drop()")
        || compact.contains("deletemany({})")
        || compact.contains(".remove({})");
    sql || mongo
}

// ─────────────────────────────────────────────────────────────────────────────
// Containers, clusters, infrastructure, cloud
// ─────────────────────────────────────────────────────────────────────────────

/// Hosting and cloud CLIs whose `delete` / `destroy` subcommands remove resources.
const CLOUD_CLIS: &[&str] = &[
    "gcloud",
    "az",
    "railway",
    "fly",
    "flyctl",
    "heroku",
    "vercel",
    "netlify",
    "supabase",
    "firebase",
    "wrangler",
    "doctl",
    "gh",
    "hcloud",
    "linode-cli",
    "scw",
    "neonctl",
    "pscale",
    "turso",
    "render",
    "vultr-cli",
    "upcloud",
    "exo",
    "oci",
    "ibmcloud",
];

/// Words that are not options, also skipping the value after an option given as a
/// separate word (`-f dev.yml`, `--profile prod`). Used to find subcommands; it can drop
/// a positional word that follows a boolean option, which is fine for that purpose.
fn command_words(args: &[String]) -> Vec<&str> {
    let mut words = Vec::new();
    let mut i = 0;
    while let Some(a) = args.get(i) {
        i += 1;
        if is_flag(a) {
            let takes_value = !a.contains('=')
                && (a.starts_with("--") || a.len() == 2)
                && args.get(i).is_some_and(|next| !next.starts_with('-'));
            if takes_value {
                i += 1;
            }
        } else {
            words.push(a.as_str());
        }
    }
    words
}

/// Read-only or non-destructive verbs of cloud CLIs; the first verb decides.
const SAFE_VERBS: &[&str] = &[
    "list", "ls", "describe", "get", "show", "view", "info", "status", "logs", "log", "tail",
    "create", "add", "new", "update", "set", "edit", "deploy", "up", "run", "exec", "ssh", "open",
    "cp", "copy", "mv", "sync", "search", "inspect", "watch", "config", "auth", "login", "logout",
    "whoami", "env", "link", "clone", "fork", "checkout", "merge", "review", "comment", "close",
    "reopen", "diff", "download", "upload", "browse", "start", "stop", "restart", "scale",
    "rollback", "promote", "build", "init", "help", "version",
];

fn infra_rules(cmd: &str, args: &[String], ops: &[&str], hits: &mut Vec<Hit>) {
    let has = |names: &[&str]| args.iter().any(|a| names.contains(&a.as_str()));
    let words = command_words(args);
    let ops: &[&str] = if matches!(
        cmd,
        "docker" | "podman" | "nerdctl" | "docker-compose" | "podman-compose" | "aws"
    ) {
        &words
    } else {
        ops
    };
    let starts = |seq: &[&str]| ops.len() >= seq.len() && ops[..seq.len()] == *seq;

    // Containers
    if matches!(cmd, "docker" | "podman" | "nerdctl") {
        let compose_down = starts(&["compose", "down"]) && (has(&["-v", "--volumes"]));
        if starts(&["system", "prune"])
            || starts(&["volume", "prune"])
            || starts(&["volume", "rm"])
            || starts(&["volume", "remove"])
            || compose_down
        {
            hits.push(hit("container.prune", "deletes container volumes or data"));
        }
    }
    if matches!(cmd, "docker-compose" | "podman-compose")
        && starts(&["down"])
        && has(&["-v", "--volumes"])
    {
        hits.push(hit("container.prune", "deletes container volumes or data"));
    }

    // Kubernetes
    if matches!(cmd, "kubectl" | "oc" | "k") {
        let broad_kind = |o: &&str| {
            let kind = o.split('/').next().unwrap_or(o).to_ascii_lowercase();
            matches!(
                kind.as_str(),
                "namespace"
                    | "namespaces"
                    | "ns"
                    | "pv"
                    | "pvc"
                    | "persistentvolume"
                    | "persistentvolumes"
                    | "persistentvolumeclaim"
                    | "persistentvolumeclaims"
                    | "crd"
                    | "crds"
                    | "customresourcedefinition"
                    | "customresourcedefinitions"
            )
        };
        let deleting = ops.contains(&"delete");
        let mass = has(&["--all", "-A", "--all-namespaces"]);
        if ops.contains(&"drain") || (deleting && (mass || ops.iter().any(broad_kind))) {
            hits.push(hit(
                "k8s.delete",
                "deletes cluster resources in bulk or with their data",
            ));
        }
    }
    if cmd == "helm"
        && matches!(
            ops.first(),
            Some(&"uninstall") | Some(&"delete") | Some(&"del") | Some(&"un")
        )
    {
        hits.push(hit("k8s.delete", "uninstalls a Helm release"));
    }

    // Infrastructure as code
    if matches!(cmd, "terraform" | "tofu" | "terragrunt") {
        let auto_apply = ops.first() == Some(&"apply")
            && has(&["-destroy", "--destroy", "-auto-approve", "--auto-approve"]);
        if ops.contains(&"destroy") || auto_apply || starts(&["state", "rm"]) {
            hits.push(hit(
                "iac.destroy",
                "destroys infrastructure or applies changes without review",
            ));
        }
    }
    if matches!(
        cmd,
        "pulumi" | "cdk" | "cdktf" | "sst" | "serverless" | "sls"
    ) && (ops.contains(&"destroy") || ops.contains(&"remove") || starts(&["stack", "rm"]))
    {
        hits.push(hit("iac.destroy", "destroys deployed infrastructure"));
    }

    // Cloud CLIs
    let lower: Vec<String> = ops.iter().map(|o| o.to_ascii_lowercase()).collect();
    let cloud = match cmd {
        "aws" => {
            // `aws <service> <operation>`: only the operation counts, not names after it.
            lower
                .get(1)
                .is_some_and(|o| o.starts_with("delete-") || o.starts_with("terminate-"))
                || (starts(&["s3", "rm"]) && has(&["--recursive"]))
                || (starts(&["s3", "sync"]) && has(&["--delete"]))
                || starts(&["s3", "rb"])
        }
        "gsutil" => {
            ops.first() == Some(&"rb")
                || (ops.first() == Some(&"rm")
                    && args
                        .iter()
                        .any(|a| a == "--recursive" || has_short_flag(a, &['r', 'R', 'a'])))
        }
        c if CLOUD_CLIS.contains(&c) => {
            let deletes = |o: &str| {
                matches!(o, "delete" | "destroy" | "pg:reset")
                    || o.ends_with(":delete")
                    || o.ends_with(":destroy")
                    || (c == "railway" && o == "down")
                    || (c == "vercel" && matches!(o, "rm" | "remove"))
                    || (c == "gcloud"
                        && o == "rm"
                        && args.iter().any(|a| a == "-r" || a == "--recursive"))
            };
            // The first verb is the action; later words are names (`describe destroy`).
            command_words(args)
                .iter()
                .map(|w| w.to_ascii_lowercase())
                .find(|w| {
                    let verb = w.rsplit(':').next().unwrap_or(w);
                    deletes(w) || SAFE_VERBS.contains(&verb)
                })
                .is_some_and(|w| deletes(&w))
        }
        _ => false,
    };
    if cloud {
        hits.push(hit("cloud.delete", "deletes cloud or hosting resources"));
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Raw HTTP
// ─────────────────────────────────────────────────────────────────────────────

/// Words in a GraphQL mutation that mean it removes something.
const DELETE_WORDS: &[&str] = &["delete", "destroy", "drop", "remove", "purge", "wipe"];

fn http_rules(cmd: &str, args: &[String], ops: &[&str], hits: &mut Vec<Hit>) {
    let method_delete = match cmd {
        "curl" => option_values(args, &['X'], &["--request"])
            .iter()
            .any(|m| m.eq_ignore_ascii_case("delete")),
        "wget" => option_values(args, &[], &["--method"])
            .iter()
            .any(|m| m.eq_ignore_ascii_case("delete")),
        "http" | "https" | "xh" | "xhs" => ops
            .first()
            .is_some_and(|m| m.eq_ignore_ascii_case("delete")),
        _ => return,
    };
    let body = args.join(" ").to_ascii_lowercase();
    let graphql_delete = body.contains("mutation") && DELETE_WORDS.iter().any(|w| body.contains(w));
    if method_delete || graphql_delete {
        hits.push(hit(
            "http.delete",
            "sends an API request that deletes something",
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule_for(command: &str, stdin: &[&str]) -> Option<&'static str> {
        let words: Vec<String> = command.split_whitespace().map(str::to_string).collect();
        let stdin: Vec<String> = stdin.iter().map(|s| s.to_string()).collect();
        check(&words[0], &words[1..], &stdin)
            .first()
            .map(|h| h.rule)
    }

    #[test]
    fn destructive_commands_table() {
        let cases: &[(&str, Option<&str>)] = &[
            // filesystem
            ("rm -rf /", Some("fs.rm-rf-broad")),
            ("rm -r ~", Some("fs.rm-rf-broad")),
            (
                "rm --recursive --no-preserve-root x",
                Some("fs.rm-rf-broad"),
            ),
            ("rm -rf target", None),
            // Windows: cmd.exe and PowerShell
            ("rmdir /s /q d:\\", Some("fs.rm-rf-broad")),
            ("RD /S /Q C:/", Some("fs.rm-rf-broad")),
            ("rd /s /q build", None),
            ("del /s /q C:\\*", Some("fs.rm-rf-broad")),
            ("del /q notes.txt", None),
            ("Remove-Item -Recurse -Force C:\\", Some("fs.rm-rf-broad")),
            ("Remove-Item -Recurse -Force $HOME", Some("fs.rm-rf-broad")),
            ("Remove-Item -Recurse .\\build", None),
            ("rm -Recurse -Force D:/", Some("fs.rm-rf-broad")),
            ("format D: /q", Some("fs.disk")),
            ("find . -name *.tmp -delete", None),
            ("find . -type d -name __pycache__ -exec rm -rf {} +", None),
            ("find . -delete", Some("fs.find-delete")),
            ("find . -type f -delete", Some("fs.find-delete")),
            ("find . -name * -delete", Some("fs.find-delete")),
            ("find / -type f -exec rm {} ;", Some("fs.find-delete")),
            ("find -name x -delete", None),
            ("find build -name *.o -delete", None),
            ("find . -name *.rs", None),
            ("chmod -R 777 /", Some("fs.chmod-broad")),
            ("chown -R me ~", Some("fs.chmod-broad")),
            ("chmod -R u+w build", None),
            ("chmod -R u+rwX .", None),
            ("chown -R $USER .", None),
            ("rm -rf ${HOME}/", Some("fs.rm-rf-broad")),
            ("rm -rf ${HOME}/*", Some("fs.rm-rf-broad")),
            ("fdisk -l /dev/sda", None),
            ("parted /dev/sda print", None),
            ("parted /dev/sda mklabel gpt", Some("fs.disk")),
            ("mkfs.ext4 /dev/sdb1", Some("fs.disk")),
            ("dd if=img of=/dev/disk2", Some("fs.disk")),
            ("dd if=/dev/zero of=/dev/null", None),
            ("wipefs -a /dev/sdb", Some("fs.disk")),
            ("diskutil eraseDisk APFS X disk2", Some("fs.disk")),
            ("diskutil list", None),
            // git
            ("git push --force", Some("git.push-force")),
            ("git push -f origin main", Some("git.push-force")),
            ("git push origin +main", Some("git.push-force")),
            ("git push --force-with-lease", None),
            ("git commit --no-verify -m x", Some("git.commit-no-verify")),
            ("git commit -nm x", Some("git.commit-no-verify")),
            ("git reset --hard", Some("git.reset-hard")),
            ("git reset --hard HEAD~3", Some("git.reset-hard")),
            (
                "git -C repo reset --hard origin/main",
                Some("git.reset-hard"),
            ),
            ("git reset --soft HEAD~1", None),
            ("git reset HEAD file.txt", None),
            ("git clean -fd", Some("git.clean-force")),
            ("git clean -xdf", Some("git.clean-force")),
            ("git clean --force", Some("git.clean-force")),
            ("git clean -n", None),
            ("git clean -fdn", None),
            ("git checkout -- .", Some("git.discard-changes")),
            ("git checkout .", Some("git.discard-changes")),
            ("git restore .", Some("git.discard-changes")),
            ("git restore --worktree :/", Some("git.discard-changes")),
            ("git checkout -f main", Some("git.discard-changes")),
            ("git restore --staged .", None),
            ("git checkout main", None),
            ("git checkout -b feature", None),
            ("git branch -D feature", Some("git.branch-force-delete")),
            (
                "git branch --delete --force feature",
                Some("git.branch-force-delete"),
            ),
            ("git branch -d feature", None),
            ("git stash drop", Some("git.stash-drop")),
            ("git stash clear", Some("git.stash-drop")),
            ("git stash pop", None),
            ("git push origin --delete feature", Some("git.push-delete")),
            ("git push origin :feature", Some("git.push-delete")),
            ("git push -d origin feature", Some("git.push-delete")),
            ("git push origin feature", None),
            (
                "git filter-branch --tree-filter x HEAD",
                Some("git.history-rewrite"),
            ),
            (
                "git filter-repo --path x --invert-paths",
                Some("git.history-rewrite"),
            ),
            (
                "git reflog expire --expire=now --all",
                Some("git.history-rewrite"),
            ),
            (
                "git update-ref -d refs/heads/x",
                Some("git.history-rewrite"),
            ),
            ("git status", None),
            // databases
            ("psql -c DROP_TABLE_PLACEHOLDER", None),
            ("dropdb production", Some("db.drop")),
            ("mysqladmin -u root drop shop", Some("db.drop")),
            ("redis-cli FLUSHALL", Some("db.redis-flush")),
            ("redis-cli -n 2 flushdb", Some("db.redis-flush")),
            ("redis-cli GET key", None),
            ("npx prisma migrate reset", None),
            ("prisma migrate reset --force", Some("db.orm-reset")),
            ("prisma db push --force-reset", Some("db.orm-reset")),
            ("prisma db push --accept-data-loss", Some("db.orm-reset")),
            ("prisma migrate dev", None),
            ("rails db:drop", Some("db.orm-reset")),
            ("rake db:reset", Some("db.orm-reset")),
            ("rails db:migrate", None),
            ("python manage.py flush --noinput", Some("db.orm-reset")),
            ("python3 manage.py migrate", None),
            ("supabase db reset", Some("db.orm-reset")),
            ("sequelize db:drop", Some("db.orm-reset")),
            // containers and clusters
            ("docker system prune -af", Some("container.prune")),
            ("docker volume prune", Some("container.prune")),
            ("docker volume rm pgdata", Some("container.prune")),
            ("docker compose down -v", Some("container.prune")),
            ("docker-compose down --volumes", Some("container.prune")),
            ("podman system prune", Some("container.prune")),
            ("docker compose down", None),
            ("docker compose -f dev.yml down -v", Some("container.prune")),
            ("docker-compose -f dev.yml down -v", Some("container.prune")),
            (
                "docker compose -p x down --volumes",
                Some("container.prune"),
            ),
            ("docker ps -a", None),
            ("kubectl delete namespace prod", Some("k8s.delete")),
            ("kubectl delete ns/prod", Some("k8s.delete")),
            ("kubectl delete pods --all", Some("k8s.delete")),
            ("kubectl delete pvc data-0", Some("k8s.delete")),
            (
                "kubectl -n prod delete deploy --all-namespaces",
                Some("k8s.delete"),
            ),
            ("kubectl drain node-1", Some("k8s.delete")),
            ("kubectl delete pod web-1", None),
            ("kubectl get pods", None),
            ("helm uninstall api", Some("k8s.delete")),
            ("helm list", None),
            // infrastructure as code
            ("terraform destroy", Some("iac.destroy")),
            ("terraform apply -destroy", Some("iac.destroy")),
            ("terraform apply -auto-approve", Some("iac.destroy")),
            ("terraform state rm aws_db.main", Some("iac.destroy")),
            ("tofu destroy -auto-approve", Some("iac.destroy")),
            ("terragrunt run-all destroy", Some("iac.destroy")),
            ("pulumi destroy --yes", Some("iac.destroy")),
            ("cdk destroy --force", Some("iac.destroy")),
            ("terraform plan", None),
            ("terraform apply", None),
            // cloud and hosting CLIs
            ("aws s3 rm s3://bucket --recursive", Some("cloud.delete")),
            ("aws s3 rb s3://bucket --force", Some("cloud.delete")),
            (
                "aws ec2 terminate-instances --instance-ids i-1",
                Some("cloud.delete"),
            ),
            (
                "aws rds delete-db-instance --db-instance-identifier db",
                Some("cloud.delete"),
            ),
            (
                "aws --profile prod dynamodb delete-table --table-name t",
                Some("cloud.delete"),
            ),
            ("aws s3 ls", None),
            (
                "aws --profile prod s3 rm s3://b --recursive",
                Some("cloud.delete"),
            ),
            ("aws --region us-east-1 s3 rb s3://b", Some("cloud.delete")),
            ("aws s3 sync . s3://b --delete", Some("cloud.delete")),
            ("aws lambda get-function --function-name delete-user", None),
            ("aws logs tail delete-user", None),
            ("aws s3 cp delete-me.txt s3://b/", None),
            ("gh issue list --search delete", None),
            ("gh pr list --label destroy", None),
            ("gcloud run services describe destroy", None),
            ("vercel logs delete", None),
            ("heroku logs -a delete", None),
            ("aws s3 rm s3://bucket/one.txt", None),
            ("gcloud sql instances delete prod", Some("cloud.delete")),
            ("gcloud projects delete my-proj", Some("cloud.delete")),
            ("gsutil rm -r gs://bucket", Some("cloud.delete")),
            ("gsutil rb gs://bucket", Some("cloud.delete")),
            ("gcloud compute instances list", None),
            ("az group delete --name rg", Some("cloud.delete")),
            ("railway volume delete", Some("cloud.delete")),
            ("railway down", Some("cloud.delete")),
            ("railway up", None),
            ("fly apps destroy api", Some("cloud.delete")),
            ("flyctl volumes destroy vol_1", Some("cloud.delete")),
            ("heroku apps:destroy --app x", Some("cloud.delete")),
            ("heroku pg:reset DATABASE", Some("cloud.delete")),
            ("heroku logs", None),
            ("vercel remove my-app --yes", Some("cloud.delete")),
            ("vercel rm my-app", Some("cloud.delete")),
            ("vercel deploy", None),
            ("netlify sites:delete id", Some("cloud.delete")),
            ("supabase projects delete ref", Some("cloud.delete")),
            (
                "firebase firestore:delete --all-collections",
                Some("cloud.delete"),
            ),
            ("wrangler d1 delete db", Some("cloud.delete")),
            ("doctl compute droplet delete web", Some("cloud.delete")),
            ("gh repo delete owner/repo --yes", Some("cloud.delete")),
            ("gh release delete v1", Some("cloud.delete")),
            ("gh pr list", None),
            // raw API calls
            (
                "curl -X DELETE https://api.example.com/v1/volumes/1",
                Some("http.delete"),
            ),
            (
                "curl --request DELETE https://api.example.com/x",
                Some("http.delete"),
            ),
            (
                "curl -XDELETE https://api.example.com/x",
                Some("http.delete"),
            ),
            (
                "curl --request=DELETE https://api.example.com/x",
                Some("http.delete"),
            ),
            (
                "wget --method=DELETE https://api.example.com/x",
                Some("http.delete"),
            ),
            ("http DELETE api.example.com/x", Some("http.delete")),
            ("xh delete api.example.com/x", Some("http.delete")),
            ("curl -X GET https://api.example.com/x", None),
            ("curl https://example.com/delete-guide", None),
            ("http GET api.example.com/x", None),
        ];
        for (command, expected) in cases {
            assert_eq!(rule_for(command, &[]), *expected, "command {command:?}");
        }
    }

    #[test]
    fn graphql_delete_mutations_ask() {
        // The shape of the PocketOS incident (April 2026): one API call deleted a
        // production volume and its backups.
        let words: Vec<String> = [
            "curl",
            "-X",
            "POST",
            "https://backboard.railway.app/graphql/v2",
            "-H",
            "Authorization: Bearer x",
            "-d",
            r#"{"query":"mutation { volumeDelete(volumeId: \"v1\") }"}"#,
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(check("curl", &words[1..], &[])[0].rule, "http.delete");

        let query: Vec<String> = ["-d", r#"{"query":"query { volumes { id } }"}"#, "https://x"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(check("curl", &query, &[]).is_empty());
    }

    #[test]
    fn sql_is_checked_in_arguments_and_on_stdin() {
        let w = |v: &[&str]| -> Vec<String> { v.iter().map(|s| s.to_string()).collect() };
        type Case<'a> = (&'a str, Vec<String>, Vec<String>, Option<&'a str>);
        let cases: Vec<Case> = vec![
            (
                "psql",
                w(&["-c", "DROP TABLE users"]),
                vec![],
                Some("db.sql"),
            ),
            (
                "psql",
                w(&["--command=drop database shop"]),
                vec![],
                Some("db.sql"),
            ),
            (
                "psql",
                w(&["-d", "app", "-c", "TRUNCATE orders;"]),
                vec![],
                Some("db.sql"),
            ),
            (
                "psql",
                w(&["-c", "DELETE FROM users"]),
                vec![],
                Some("db.sql"),
            ),
            (
                "psql",
                w(&["-c", "delete from users where id = 3"]),
                vec![],
                None,
            ),
            ("psql", w(&["-c", "SELECT * FROM users"]), vec![], None),
            (
                "psql",
                w(&["app"]),
                w(&["BEGIN;\nDROP SCHEMA public CASCADE;\nCOMMIT;"]),
                Some("db.sql"),
            ),
            ("psql", w(&["app"]), w(&["SELECT 1;"]), None),
            ("mysql", w(&["-e", "drop table t"]), vec![], Some("db.sql")),
            (
                "mysql",
                w(&["--execute=ALTER TABLE t DROP COLUMN email"]),
                vec![],
                Some("db.sql"),
            ),
            (
                "mariadb",
                w(&["-e", "DELETE FROM t WHERE 1=1; DELETE FROM u"]),
                vec![],
                Some("db.sql"),
            ),
            (
                "sqlite3",
                w(&["app.db", "DROP TABLE users"]),
                vec![],
                Some("db.sql"),
            ),
            ("sqlite3", w(&["app.db", ".tables"]), vec![], None),
            ("duckdb", w(&["-c", "drop table x"]), vec![], Some("db.sql")),
            (
                "clickhouse-client",
                w(&["--query", "TRUNCATE TABLE logs"]),
                vec![],
                Some("db.sql"),
            ),
            (
                "mongosh",
                w(&["--eval", "db.dropDatabase()"]),
                vec![],
                Some("db.sql"),
            ),
            (
                "mongosh",
                w(&["--eval", "db.users.deleteMany({})"]),
                vec![],
                Some("db.sql"),
            ),
            ("mongosh", w(&["--eval", "db.users.find({})"]), vec![], None),
            // SQL-looking text for a program that is not a database client is data.
            ("echo", w(&["DROP TABLE users"]), vec![], None),
            ("cat", w(&[]), w(&["DROP TABLE users;"]), None),
        ];
        for (cmd, args, stdin, expected) in cases {
            assert_eq!(
                check(cmd, &args, &stdin).first().map(|h| h.rule),
                expected,
                "{cmd} {args:?} stdin={stdin:?}"
            );
        }
    }

    #[test]
    fn every_hit_names_a_listed_rule_and_reads_as_a_sentence_fragment() {
        let ids: Vec<&str> = RULES.iter().map(|(id, _)| *id).collect();
        let mut seen = std::collections::BTreeSet::new();
        for (id, _) in RULES {
            assert!(seen.insert(*id), "duplicate rule {id}");
            assert!(id.contains('.'), "{id} is <category>.<name>");
        }
        for (cmd, args) in [
            ("git", "reset --hard"),
            ("terraform", "destroy"),
            ("curl", "-X DELETE https://x"),
            ("rm", "-rf /"),
        ] {
            let args: Vec<String> = args.split(' ').map(str::to_string).collect();
            for hit in check(cmd, &args, &[]) {
                assert!(ids.contains(&hit.rule), "{} is not in RULES", hit.rule);
                assert!(hit.why.starts_with("this command "), "{}", hit.why);
                assert!(!hit.why.ends_with('.'), "{}", hit.why);
            }
        }
    }
}
