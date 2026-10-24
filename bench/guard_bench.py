#!/usr/bin/env python3
"""Benchmark harness for shell-command guards exposed as Claude Code PreToolUse hooks.

Runs every case in a corpus TSV through `--guard`, classifies the hook reply
(allow / ask / block / error / timeout / malformed), repeats each case to verify
determinism, and writes results.jsonl + summary.md via --out DIR.

Python 3 stdlib only. See bench/README.md for usage and the frozen decision rule.
"""

import argparse
import json
import math
import os
import shlex
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_CORPUS = REPO_ROOT / "crates/reflex-hooks/tests/fixtures/guard_corpus.tsv"
SCOPE_SECTIONS = {"Protected writes", "Secret reads"}
VERDICTS = {"allow", "ask", "block"}
ERROR_CLASSES = {"error", "timeout", "malformed"}
DECISION_TO_VERDICT = {"deny": "block", "ask": "ask", "allow": "allow"}


class Case:
    __slots__ = ("line", "section", "expected", "command")

    def __init__(self, line, section, expected, command):
        self.line = line
        self.section = section
        self.expected = expected
        self.command = command


def parse_corpus(path):
    cases = []
    section = "preamble"
    for lineno, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        stripped = raw.strip()
        if not stripped:
            continue
        if stripped.startswith("#"):
            if stripped.startswith("# ──"):
                name = stripped.lstrip("#").strip(" ─").strip()
                if name:
                    section = name
            continue
        parts = raw.split("\t", 2)
        if len(parts) != 3:
            raise SystemExit(f"{path}:{lineno}: expected '<verdict>\\t<rule>\\t<command>'")
        verdict, _rule, command = (p.strip() for p in parts)
        if verdict not in VERDICTS:
            raise SystemExit(f"{path}:{lineno}: unknown verdict {verdict!r}")
        cases.append(Case(lineno, section, verdict, command.replace("\\n", "\n")))
    return cases


def resolve_guard(guard_string):
    argv = shlex.split(guard_string)
    if not argv:
        raise SystemExit("--guard is empty")
    # Resolve path-like argv[0] against the launch cwd; per-run cwd is a temp dir.
    if "/" in argv[0]:
        argv[0] = os.path.abspath(argv[0])
        if not os.path.isfile(argv[0]):
            raise SystemExit(f"guard binary not found: {argv[0]}")
    return argv


def classify(returncode, stdout, ms):
    if returncode == 2:
        return {"got": "block", "class": "block", "exit_code": returncode, "ms": ms}
    if returncode != 0:
        return {"got": None, "class": "error", "exit_code": returncode, "ms": ms}
    out = stdout.strip()
    if not out:
        return {"got": "allow", "class": "allow", "exit_code": returncode, "ms": ms}
    try:
        reply = json.loads(out)
    except json.JSONDecodeError:
        return {"got": None, "class": "malformed", "exit_code": returncode, "ms": ms}
    if not isinstance(reply, dict):
        return {"got": None, "class": "malformed", "exit_code": returncode, "ms": ms}
    hook_out = reply.get("hookSpecificOutput")
    decision = hook_out.get("permissionDecision") if isinstance(hook_out, dict) else None
    if decision in DECISION_TO_VERDICT:
        got = DECISION_TO_VERDICT[decision]
        return {"got": got, "class": got, "exit_code": returncode, "ms": ms}
    # Valid JSON reply without a permissionDecision: Claude Code treats it as no objection.
    return {"got": "allow", "class": "allow", "exit_code": returncode, "ms": ms}


def run_once(guard_argv, command, timeout):
    """Run the guard once in full isolation; returns got/class/exit_code/ms."""
    proj = tempfile.mkdtemp(prefix="guard-bench-proj-")
    home = tempfile.mkdtemp(prefix="guard-bench-home-")
    try:
        subprocess.run(["git", "init", "-q"], cwd=proj, capture_output=True)
        command = command.replace("/work/proj", proj).replace("/home/me", home)
        payload = {
            "session_id": "bench",
            "transcript_path": "/dev/null",
            "cwd": proj,
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": command},
        }
        env = dict(os.environ, HOME=home)
        data = json.dumps(payload).encode("utf-8")
        t0 = time.monotonic()
        try:
            proc = subprocess.run(
                guard_argv, input=data, cwd=proj, env=env, capture_output=True,
                timeout=timeout,
            )
        except subprocess.TimeoutExpired:
            return {"got": None, "class": "timeout", "exit_code": None,
                    "ms": round((time.monotonic() - t0) * 1000, 1)}
        ms = round((time.monotonic() - t0) * 1000, 1)
        return classify(proc.returncode, proc.stdout.decode("utf-8", "replace"), ms)
    finally:
        shutil.rmtree(proj, ignore_errors=True)
        shutil.rmtree(home, ignore_errors=True)


def run_bench(args, cases, guard_argv, name):
    out_file = None
    if args.out:
        args.out.mkdir(parents=True, exist_ok=True)
        out_file = (args.out / "results.jsonl").open("w", encoding="utf-8")
    records = []
    try:
        for i, case in enumerate(cases, 1):
            print(f"[{i}/{len(cases)}] line {case.line}: {case.section}", file=sys.stderr)
            for rep in range(1, args.reps + 1):
                res = run_once(guard_argv, case.command, args.timeout)
                rec = {
                    "name": name,
                    "rep": rep,
                    "line": case.line,
                    "section": case.section,
                    "command": case.command,
                    "expected": case.expected,
                    "got": res["got"],
                    "class": res["class"],
                    "exit_code": res["exit_code"],
                    "ms": res["ms"],
                }
                records.append(rec)
                if out_file:
                    out_file.write(json.dumps(rec, ensure_ascii=False) + "\n")
                    out_file.flush()
    finally:
        if out_file:
            out_file.close()
    return records


def p90(sorted_ms):
    if not sorted_ms:
        return 0.0
    return sorted_ms[max(0, math.ceil(0.9 * len(sorted_ms)) - 1)]


def aggregate(cases, by_case, records, sections):
    sel_cases = [c for c in cases if c.section in sections]
    sel_recs = [r for r in records if r["section"] in sections]
    nondet = []
    mismatches = []
    exact = 0
    danger_caught = 0
    safe_passed = 0
    n_danger = 0
    n_safe = 0
    for c in sel_cases:
        runs = by_case[c.line]
        if len({r["class"] for r in runs}) > 1:
            nondet.append((c, [r["class"] for r in runs]))
        if all(r["got"] == c.expected for r in runs):
            exact += 1
        else:
            got = "/".join(str(r["got"]) for r in runs)
            mismatches.append(f"line {c.line}: expected={c.expected} got={got}")
        if c.expected in ("ask", "block"):
            n_danger += 1
            if all(r["got"] in ("ask", "block") for r in runs):
                danger_caught += 1
        else:
            n_safe += 1
            if all(r["got"] == "allow" for r in runs):
                safe_passed += 1
    errors = Counter(r["class"] for r in sel_recs if r["class"] in ERROR_CLASSES)
    lat = sorted(r["ms"] for r in sel_recs)
    median = statistics.median(lat) if lat else 0.0
    return {
        "n_cases": len(sel_cases),
        "n_runs": len(sel_recs),
        "exact": exact,
        "n_danger": n_danger,
        "danger_caught": danger_caught,
        "n_safe": n_safe,
        "safe_passed": safe_passed,
        "errors": errors,
        "median_ms": median,
        "p90_ms": p90(lat),
        "nondet": nondet,
        "mismatches": mismatches,
    }


def fmt_stats(st, reps):
    lines = [
        f"- cases: {st['n_cases']} ({st['n_runs']} runs)",
        f"- exact verdict match: {st['exact']}/{st['n_cases']}",
        f"- dangerous caught: {st['danger_caught']}/{st['n_danger']}"
        " (expected ask|block → got ask|block)",
        f"- safe passed: {st['safe_passed']}/{st['n_safe']} (expected allow → got allow)",
        "- errors by class: error={}, timeout={}, malformed={}".format(
            st["errors"]["error"], st["errors"]["timeout"], st["errors"]["malformed"]),
        f"- latency: median {st['median_ms']:.1f} ms, p90 {st['p90_ms']:.1f} ms",
    ]
    if st["nondet"]:
        shown = "; ".join(
            f"line {c.line} ({'/'.join(classes)})" for c, classes in st["nondet"][:10])
        lines.append(f"- determinism: FAIL (reps differ) — {shown}")
    else:
        lines.append(f"- determinism: PASS ({reps} reps identical)")
    for m in st["mismatches"][:15]:
        lines.append(f"  - mismatch: {m}")
    if len(st["mismatches"]) > 15:
        lines.append(f"  - … and {len(st['mismatches']) - 15} more")
    return lines


def build_summary(name, args, cases, records):
    by_case = {}
    for r in records:
        by_case.setdefault(r["line"], []).append(r)
    sections = list(dict.fromkeys(c.section for c in cases))
    scored = [s for s in sections if s not in SCOPE_SECTIONS]
    scope = [s for s in sections if s in SCOPE_SECTIONS]

    total = aggregate(cases, by_case, records, set(sections))
    lines = [
        f"# Guard benchmark — {name}",
        "",
        f"- corpus: `{args.corpus}` — {total['n_cases']} cases",
        f"- reps: {args.reps} · timeout: {args.timeout:g}s · "
        f"date: {datetime.now(timezone.utc).isoformat(timespec='seconds')}",
        f"- exact verdict match: **{total['exact']}/{total['n_cases']}** "
        "(all reps identical and equal to expected)",
        f"- dangerous caught: {total['danger_caught']}/{total['n_danger']} · "
        f"safe passed: {total['safe_passed']}/{total['n_safe']}",
        "- errors by class: error={}, timeout={}, malformed={}".format(
            total["errors"]["error"], total["errors"]["timeout"],
            total["errors"]["malformed"]),
        f"- latency: median {total['median_ms']:.1f} ms, p90 {total['p90_ms']:.1f} ms",
        "- determinism: " + ("FAIL" if total["nondet"] else
                             f"PASS ({args.reps} reps identical)"),
        "",
        "## Scope (reported separately)",
        "",
        "The protected-write and secret-read sections are outside the decision "
        "rule: not every guard claims them. Totals across scope sections "
        f"({', '.join(scope)}):",
        "",
    ]
    lines += fmt_stats(aggregate(cases, by_case, records, set(scope)), args.reps)
    lines += [
        "",
        "## Scored total (excludes scope sections)",
        "",
    ]
    lines += fmt_stats(aggregate(cases, by_case, records, set(scored)), args.reps)
    lines += ["", "## Per section", ""]
    for section in sections:
        st = aggregate(cases, by_case, records, {section})
        lines.append(f"### {section}")
        lines.append("")
        lines += fmt_stats(st, args.reps)
        lines.append("")
    return "\n".join(lines)


def selftest():
    """Verify the harness itself with fake guards (sh -c one-liners)."""
    def json_out(decision):
        payload = json.dumps({"hookSpecificOutput": {
            "hookEventName": "PreToolUse", "permissionDecision": decision}})
        return f"printf '{payload}'"

    deny_on_rmrf = 'input=$(cat); case "$input" in *"rm -rf"*) exit 2;; *) exit 0;; esac'
    cases = [
        ("always-allow (exit 0, no output)", ["sh", "-c", "exit 0"],
         "cargo test", "allow"),
        ("always exit 2", ["sh", "-c", "exit 2"], "cargo test", "block"),
        ("JSON ask", ["sh", "-c", json_out("ask")], "cargo test", "ask"),
        ("JSON deny", ["sh", "-c", json_out("deny")], "cargo test", "block"),
        ("JSON allow", ["sh", "-c", json_out("allow")], "cargo test", "allow"),
        ("crash (exit 1 → error)", ["sh", "-c", "exit 1"], "cargo test", "error"),
        ("malformed stdout", ["sh", "-c", "printf 'not json'"], "cargo test",
         "malformed"),
        ("sleep past timeout", ["sh", "-c", "sleep 5"], "cargo test", "timeout"),
        ("stdin passthrough: rm -rf denied", ["sh", "-c", deny_on_rmrf],
         "rm -rf /tmp/guard-bench-selftest", "block"),
        ("stdin passthrough: safe command allowed", ["sh", "-c", deny_on_rmrf],
         "ls -la", "allow"),
    ]
    ok = True
    print(f"{'case':45s} {'want':10s} {'got':10s} result")
    for label, argv, command, want in cases:
        res = run_once(argv, command, timeout=2.0)
        passed = res["class"] == want
        ok = ok and passed
        print(f"{label:45s} {want:10s} {res['class']:10s} "
              f"{'PASS' if passed else 'FAIL'}")
    print("selftest:", "PASS" if ok else "FAIL")
    return 0 if ok else 1


def parse_args(argv):
    ap = argparse.ArgumentParser(
        description="Benchmark a PreToolUse shell-command guard against a corpus TSV.")
    ap.add_argument("--guard", help="guard command line (shlex-split), e.g. "
                    "'target/release/reflex hook claude-code pre-tool'")
    ap.add_argument("--corpus", type=Path, default=DEFAULT_CORPUS,
                    help="corpus TSV (default: %(default)s)")
    ap.add_argument("--out", type=Path, help="write results.jsonl and summary.md here")
    ap.add_argument("--name", help="label for the guard in outputs (default: guard string)")
    ap.add_argument("--reps", type=int, default=3,
                    help="repetitions per case for the determinism check (default: %(default)s)")
    ap.add_argument("--timeout", type=float, default=10.0,
                    help="per-run timeout in seconds (default: %(default)s)")
    ap.add_argument("--selftest", action="store_true",
                    help="test the harness itself with fake guards")
    return ap.parse_args(argv)


def main(argv=None):
    args = parse_args(argv)
    if args.selftest:
        return selftest()
    if not args.guard:
        raise SystemExit("--guard is required (or use --selftest)")
    guard_argv = resolve_guard(args.guard)
    name = args.name or args.guard
    cases = parse_corpus(args.corpus)
    records = run_bench(args, cases, guard_argv, name)
    summary = build_summary(name, args, cases, records)
    print(summary)
    if args.out:
        (args.out / "summary.md").write_text(summary + "\n", encoding="utf-8")
        print(f"\nwrote {args.out / 'results.jsonl'} and {args.out / 'summary.md'}",
              file=sys.stderr)
    by_case = {}
    for r in records:
        by_case.setdefault(r["line"], []).append(r)
    nondet = [c for c in cases
              if len({r["class"] for r in by_case[c.line]}) > 1]
    if nondet:
        print(f"DETERMINISM FAILURE: {len(nondet)} case(s) differ across reps: "
              + ", ".join(f"line {c.line}" for c in nondet), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
