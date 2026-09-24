# Lessons

Patterns extracted from corrections and mistakes. Each entry is a rule that prevents a repeat.

---

## L1 — Never pipe a backgrounded command through `tail`

**Mistake (2026-09-24):** Ran `cargo test --workspace 2>&1 | tail -60` in the background, then grepped the
output file for `test result:` lines and reported **14 tests**. The real total was **53** — the output file
only ever held the last 60 lines, so five crates were invisible.

**Rule:** When a command's output will be parsed later, redirect the *full* stream to a file
(`cmd > /tmp/out.txt 2>&1`) and trim only at read time. `tail` in the pipeline destroys the data before it
is ever stored.

**Generalization:** Any filter placed *before* persistence is irreversible. Filter at the point of reading,
never at the point of writing.

---

## L2 — `git branch --merged` lies after a squash merge

**Context (2026-09-24):** `codex/release-evidence-hardening` had two commits absent from `main`, and `main`
had one commit absent from the branch. Commit-ancestry tools reported it as unmerged; it had in fact been
squash-merged, so its content was fully present.

**Rule:** To decide whether a branch is safe to delete, compare **content**, not ancestry:

```bash
git diff --quiet main <branch> && echo "content identical — safe to delete"
```

Record the tip SHA before deleting (`git rev-parse <branch>`) so the branch can be restored with
`git branch <name> <sha>` if the judgment was wrong.

---

## L3 — Verify a documented claim before repeating it

**Context (2026-09-24):** `fixtures/README.md` stated the evaluation split shares "21 contexts with
development, 21 with validation, 22 with calibration." Measuring it directly confirmed those pairwise
numbers but surfaced the figure that actually mattered and was *not* written down: **30 of 32** distinct
contexts appear in some other split, leaving only **2** genuinely held out.

**Rule:** A repo's own prose is a lead, not evidence. Recompute before building a recommendation on it —
the aggregate that matters is often absent from the documented per-pair breakdown.

---

## L4 — Version truth lives in three places and drifts silently

**Context (2026-09-24):** `Cargo.toml` said `0.1.1` and tag `v0.1.1` was pushed, but `CHANGELOG.md` stopped
at `0.1.0`. No test checks that these agree, so nothing caught it.

**Rule:** When touching a release, check all three together: `Cargo.toml` version, the newest git tag, and
the top `## ` heading in `CHANGELOG.md`.
