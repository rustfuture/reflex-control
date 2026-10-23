#!/usr/bin/env python3
"""False-alarm and false-friend measurement on the commands agents really ran locally.

Reads the shell commands out of local agent transcripts (Claude Code and Codex), feeds
each one to `reflex check --` in a throwaway project with a throwaway HOME, and reports
that reflex would ask/block how many of them. The commands stay on this machine: only
counts and the flagged commands go into the report.

A `reflex check` run costs a process start (~7 ms), so the transcript commands are
checked in parallel with a thread pool and the whole thing takes seconds.

Python 3 stdlib only.

    python3 bench/traffic.py --reflex target/release/reflex --out /tmp/traffic
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path


def claude_commands(root):
    """Every Bash tool call in Claude Code's transcripts (`~/.claude/projects/**/*.jsonl`)."""
    seen = Counter()
    sessions = 0
    for path in sorted(root.rglob("*.jsonl")):
        sessions += 1
        try:
            lines = path.read_text(errors="replace").splitlines()
        except OSError:
            continue
        for line in lines:
            try:
                entry = json.loads(line)
            except json.JSONDecodeError:
                continue
            message = entry.get("message") if isinstance(entry, dict) else None
            content = message.get("content") if isinstance(message, dict) else None
            if not isinstance(content, list):
                continue
            for block in content:
                if (
                    isinstance(block, dict)
                    and block.get("type") == "tool_use"
                    and block.get("name") == "Bash"
                ):
                    command = (block.get("input") or {}).get("command")
                    if isinstance(command, str) and command.strip():
                        seen[command] += 1
    return sessions, seen


CODEX_SHELL_CALLS = {
    "exec_command",
    "shell",
    "shell_command",
    "local_shell_call",
    "exec",
}


def codex_arguments(payload):
    """`cmd` for a Codex shell call, whether it is a string, a list or a JSON string."""
    raw = payload.get("arguments")
    if isinstance(raw, str):
        try:
            raw = json.loads(raw)
        except json.JSONDecodeError:
            raw = None
    candidates = [raw, payload.get("action"), payload.get("input")]
    for candidate in candidates:
        if not isinstance(candidate, dict):
            continue
        for key in ("cmd", "command"):
            value = candidate.get(key)
            if isinstance(value, str):
                return value
            if isinstance(value, list) and all(isinstance(v, str) for v in value):
                return " ".join(value)
    return None


def codex_commands(root):
    """Every shell call in Codex's rollout files (`~/.codex/sessions/**/*.jsonl`)."""
    seen = Counter()
    sessions = 0
    for path in sorted(root.rglob("*.jsonl")):
        sessions += 1
        try:
            lines = path.read_text(errors="replace").splitlines()
        except OSError:
            continue
        for line in lines:
            try:
                entry = json.loads(line)
            except json.JSONDecodeError:
                continue
            payload = entry.get("payload") if isinstance(entry, dict) else None
            if not isinstance(payload, dict):
                continue
            name = payload.get("name") or payload.get("type")
            if name not in CODEX_SHELL_CALLS:
                continue
            command = codex_arguments(payload)
            if command and command.strip():
                seen[command] += 1
    return sessions, seen


def check(reflex, project, home, command):
    env = dict(os.environ, HOME=str(home), PYTHONDONTWRITEBYTECODE="1")
    proc = subprocess.run(
        [reflex, "check", "--", command],
        cwd=project,
        env=env,
        capture_output=True,
        text=True,
        timeout=120,
    )
    # 0 allow, 1 ask, 2 block, 3 the check itself failed
    kind = {0: "allow", 1: "ask", 2: "block", 3: "error"}.get(proc.returncode)
    return kind or f"exit {proc.returncode}"


def reflex_version(reflex):
    try:
        out = subprocess.run([reflex, "--version"], capture_output=True, text=True, timeout=30)
        return out.stdout.strip().splitlines()[0] if out.stdout.strip() else "?"
    except OSError:
        return "?"


def main():
    home = Path.home()
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reflex", default="target/release/reflex")
    parser.add_argument("--out", required=True, help="directory for the report and the flagged commands")
    parser.add_argument("--claude", default=str(home / ".claude" / "projects"))
    parser.add_argument("--codex", default=str(home / ".codex" / "sessions"))
    parser.add_argument("--jobs", type=int, default=8)
    parser.add_argument(
        "--no-codex",
        action="store_true",
        help="skip Codex transcripts (they run to gigabytes)",
    )
    args = parser.parse_args()

    reflex = str(Path(args.reflex).resolve())
    if not Path(reflex).is_file():
        sys.exit(f"no binary at {reflex}; run `cargo build --release` first")
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)

    sources = {}
    if Path(args.claude).is_dir():
        sessions, commands = claude_commands(Path(args.claude))
        # The same command typed again is the same decision: count it once.
        sources["Claude Code"] = (sessions, commands)
    if not args.no_codex and Path(args.codex).is_dir():
        sessions, commands = codex_commands(Path(args.codex))
        sources["Codex"] = (sessions, commands)
    if not sources:
        sys.exit("no transcripts found")

    united = Counter()
    for _, commands in sources.values():
        united.update(commands)

    project = Path(tempfile.mkdtemp(prefix="reflex-traffic-project-"))
    fake_home = Path(tempfile.mkdtemp(prefix="reflex-traffic-home-"))
    try:
        subprocess.run(["git", "init", "-q"], cwd=project, check=True, capture_output=True)
        work = list(united)
        with ThreadPoolExecutor(max_workers=args.jobs) as pool:
            verdicts = list(pool.map(lambda c: check(reflex, project, fake_home, c), work))
    finally:
        shutil.rmtree(project, ignore_errors=True)
        shutil.rmtree(fake_home, ignore_errors=True)
    verdict_of = dict(zip(work, verdicts))

    lines = []
    lines.append("# Reflex Control on real agent traffic")
    lines.append("")
    lines.append(f"- reflex: `{reflex_version(reflex)}`")
    lines.append("- settings: built-in defaults (a fresh `git init` project, no `.reflex.toml`)")
    lines.append("- comparison: exact command text, duplicates counted once per source")
    lines.append("")
    lines.append("| source | transcripts | shell calls | distinct | allow | ask | block | error |")
    lines.append("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |")
    totals = Counter()
    for name, (sessions, commands) in sources.items():
        counts = Counter(verdict_of[c] for c in commands)
        calls = sum(commands.values())
        lines.append(
            f"| {name} | {sessions} | {calls} | {len(commands)} | {counts['allow']} | "
            f"{counts['ask']} | {counts['block']} | {counts['error']} |"
        )
        for key in ("allow", "ask", "block", "error"):
            totals[key] += counts[key]
    lines.append("")
    flagged = {c: v for c, v in verdict_of.items() if v != "allow"}
    lines.append(
        f"Distinct commands across both: {len(united)}; {len(flagged)} would stop "
        f"({totals['ask']} ask, {totals['block']} block, {totals['error']} error, "
        f"counting a command once per source)."
    )
    lines.append("")
    lines.append("## Flagged commands (ask or block), most repeated first")
    lines.append("")
    lines.append("```")
    for command, verdict in sorted(
        flagged.items(), key=lambda item: (-united[item[0]], item[1], item[0])
    ):
        one_line = command.replace("\n", "\\n")
        lines.append(f"{united[command]:5}x {verdict:5} {one_line}")
    lines.append("```")
    (out / "traffic.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines[:12]))
    print(f"wrote {out / 'traffic.md'}")


if __name__ == "__main__":
    main()
