#!/usr/bin/env python3
"""End-to-end latency of the Claude Code hook, the way an agent feels it.

Each sample starts `reflex hook claude-code pre-tool` as a new process, writes the
PreToolUse JSON on stdin and waits for the reply, so the number includes process start,
project/config lookup and the guard itself. The guard runs in a throwaway `git init`
project with a throwaway HOME and no `.reflex.toml`, i.e. the defaults a new user gets.

Python 3 stdlib only.

    python3 bench/latency.py --reflex target/release/reflex --runs 100
"""

import argparse
import json
import os
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

CASES = [
    ("git status", "allow"),
    ("git reset --hard HEAD", "ask"),
    ("cat .env", "block"),
]


def percentile(values, p):
    """Nearest-rank percentile over a sorted list."""
    if not values:
        return 0.0
    k = max(1, min(len(values), int(round(p / 100.0 * len(values) + 0.5)) - 1))
    return values[k]


def verdict_of(exit_code, stdout):
    stdout = stdout.strip()
    if exit_code == 2:
        return "block"
    if exit_code != 0:
        return f"exit {exit_code}"
    if not stdout:
        return "allow"
    try:
        reply = json.loads(stdout)
    except json.JSONDecodeError:
        return "malformed"
    decision = reply.get("hookSpecificOutput", {}).get("permissionDecision")
    return {"deny": "block", "ask": "ask", "allow": "allow"}.get(decision, "unknown")


def run_once(reflex, project, home, command):
    payload = {
        "session_id": "latency",
        "transcript_path": "/dev/null",
        "cwd": str(project),
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": {"command": command},
    }
    env = dict(os.environ, HOME=str(home))
    started = time.perf_counter()
    proc = subprocess.run(
        [reflex, "hook", "claude-code", "pre-tool"],
        input=json.dumps(payload),
        capture_output=True,
        text=True,
        cwd=project,
        env=env,
        timeout=30,
    )
    elapsed_ms = (time.perf_counter() - started) * 1000.0
    return elapsed_ms, verdict_of(proc.returncode, proc.stdout)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--reflex",
        default="target/release/reflex",
        help="reflex binary (default: target/release/reflex)",
    )
    parser.add_argument("--runs", type=int, default=100, help="samples per case (default 100)")
    parser.add_argument("--warmup", type=int, default=5, help="discarded runs per case (default 5)")
    parser.add_argument(
        "--json", metavar="FILE", help="also write the per-case summary as JSON"
    )
    args = parser.parse_args()

    reflex = str(Path(args.reflex).resolve())
    if not Path(reflex).is_file():
        sys.exit(f"no binary at {reflex}; run `cargo build --release` first")

    project = Path(tempfile.mkdtemp(prefix="reflex-latency-project-"))
    home = Path(tempfile.mkdtemp(prefix="reflex-latency-home-"))
    summary = []
    try:
        subprocess.run(
            ["git", "init", "-q"], cwd=project, check=True, capture_output=True
        )
        first = {}
        for command, expected in CASES:
            for _ in range(args.warmup):
                run_once(reflex, project, home, command)
            samples = []
            got = None
            for i in range(args.runs):
                elapsed, got = run_once(reflex, project, home, command)
                samples.append(elapsed)
                if i == 0:
                    first[command] = elapsed
            if got != expected:
                sys.exit(f"{command!r}: expected {expected}, hook replied {got}")
            samples.sort()
            summary.append(
                {
                    "command": command,
                    "expected": expected,
                    "n": len(samples),
                    "first_ms": round(first[command], 1),
                    "mean_ms": round(statistics.fmean(samples), 1),
                    "min_ms": round(samples[0], 1),
                    "p50_ms": round(percentile(samples, 50), 1),
                    "p90_ms": round(percentile(samples, 90), 1),
                    "p95_ms": round(percentile(samples, 95), 1),
                    "p99_ms": round(percentile(samples, 99), 1),
                    "max_ms": round(samples[-1], 1),
                }
            )
    finally:
        shutil.rmtree(project, ignore_errors=True)
        shutil.rmtree(home, ignore_errors=True)

    columns = ["command", "n", "first", "mean", "p50", "p90", "p95", "p99", "max"]
    print("| " + " | ".join(columns) + " |")
    print("|" + "|".join(["---"] + ["---:"] * (len(columns) - 1)) + "|")
    for row in summary:
        print(
            "| {command} | {n} | {first_ms} | {mean_ms} | {p50_ms} | {p90_ms} | "
            "{p95_ms} | {p99_ms} | {max_ms} |".format(**row)
        )
    print()
    print("milliseconds, one fresh process per sample, warm page cache; `first` is the")
    print("first sample of that case after the warm-up.")
    if args.json:
        Path(args.json).write_text(json.dumps(summary, indent=2) + "\n")
        print(f"wrote {args.json}")


if __name__ == "__main__":
    main()
