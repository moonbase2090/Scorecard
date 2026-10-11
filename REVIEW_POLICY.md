# Review policy

This policy applies to every pull request, whether a person or an agent wrote it.

## 1. Classify every PR

**Trunk**: the change touches shared code that other parts depend on:
- core libraries and shared modules
- auth, secrets, and permissions
- data models, schemas, and migrations
- CI, release, and signing workflows; build configuration
- public APIs, CLI flags, config formats
- anything with 5 or more dependents

**Leaf**: everything else, such as a single UI screen, docs, a self-contained script, or test-only changes.

A PR that is partly trunk is trunk. When unsure, call it trunk. Label trunk PRs `trunk`.

## 2. Proof (every PR)

The PR body has a **Proof** section with real evidence: test output, a CI run link, a screenshot or recording for UI changes, or a before/after for behavior changes. "Tested locally" without output is not proof.

## 3. Leaf PRs

- CI green and proof present.
- One review by anyone other than the author (person or agent).
- Merge once green.

## 4. Trunk PRs

- CI green, and the proof shows the change running, not just compiling.
- Agentic validation: an agent other than the author builds and exercises the change.
- Independent review, preferably by a different model than the author.
- Every finding is fixed or explicitly waived in the PR thread.
- Then merge.

## 5. Feature gating

New behavior in trunk code ships behind a flag or setting that is off by default, unless it is a pure fix. Turning a flag on by default is its own PR, and that PR is trunk.

## 6. Product quality bar

Every PR meets these. Reviewers block on any miss.

1. Simple by default: the common case works with no flags and no config. No step needs reading source code or hand-creating files.
2. Every error or skip message says what happened, why, and the exact command or config line that fixes it.
3. Docs ship in the same PR: any PR that changes CLI surface, defaults, output, or config updates the README, the reference, and the examples in that PR. `scripts/check-docs.py` runs the examples and fails on commands or flags that no longer exist.
4. UX proof in the PR body: terminal output before and after, and report screenshots if the report changed, from a real project run.
5. The first-run path (install, `sc setup`, `sc analyze`, reading the report) is walked end to end before merge for any change that touches it.
6. Output is honest: never show a number `sc` did not measure, such as 0% for unmeasured coverage. Say "not measured".
7. Smallest diff that fully solves the problem: no speculative features, no unused options, no drive-by refactors.
8. No duplicated logic. Reuse or extract, and delete dead code you touch.
9. Every new flag or config key earns its place. Defaults beat options.
10. Docs are tight and exact: no filler, no marketing tone, no restating the obvious.
11. Every claim in a PR body or doc is verified by a command you ran.
12. Tests assert real behavior (no snapshot-everything, no tautologies), and each one fails without the fix. For every test, ask: would it fail if the behavior it names broke? If not, reject it. `sc analyze --mutation diff` runs cargo-mutants and reports each mutant the tests miss (`mutation.survivor`). Reject tests that:
    - assert a value against itself, or a constant against the same constant
    - compute the expected value with the code under test or a copy of its logic
    - mock or stub the unit under test, then assert what the mock returns
    - only check that something ran, was called, or didn't panic, with no assertion on the result
    - compare against a snapshot or golden file regenerated from current output without review
    - still pass when the implementation is deleted or replaced with a stub or default
13. `sc` passes its own strict gates on this repository, with no new suppressions.

## 7. Merging

- Merge commits only. No admin overrides.
- Code-scanning threads are resolved only when they are report-only or addressed, never just to unblock a merge.
- Releases and tags need owner approval. Release text and artifacts must not reference private infrastructure, hostnames, personal paths, or internal tooling (`RELEASING.md`, `scripts/check-release-text.sh`).
