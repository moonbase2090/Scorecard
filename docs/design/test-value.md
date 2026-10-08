# Test value (design)

Status: **proposal** — report-only until MB2090 signs off on gating.  
Audience: contributors implementing engines and agents reading scorecards.

Research: [testing antipatterns](../research/testing-antipatterns.md) (catalog, citations, ranked build shortlist).

## Problem

`sc` already answers whether tests **passed**, how much code is **covered**, and which functions are **complex but untested** (CRAP). Those signals treat every passing test equally. A tautological test that re-implements the function under test can yield full line coverage and a green `tests` gate while catching no real bugs. Agent-written suites often restate the author's reading of intent; when a test fails, either the test or the product can be edited until green.

**Test value** is a family of report-only checks that estimate whether the suite would have caught planted defects in the changed code, whether the diff weakened tests, and which tests cost time without contributing signal.

## Goals

1. On a change (especially with `--diff`), report an honest **mutation score** for touched functions when a mutation tool exists.
2. Flag **test weakening** in the same diff as product changes (removed assertions, loosened expectations, new `ignore`, and similar).
3. Flag **tautological or empty** tests where detectable (Rust first).
4. Eventually rank **per-test cost vs. unique mutants killed** and summarize **unit vs integration vs e2e** mix.

Non-goals for the first releases: failing CI by default, running mutation on every file in huge repos without a budget, or replacing human judgment about product intent.

## Scorecard contract

Add a top-level JSON object `test_value` (sibling to `mutation`, `crap`, `spec`, `llm`). It aggregates sub-signals; each sub-signal has its own `status` and optional metrics. Old scorecards without `test_value` remain valid.

```json
{
  "test_value": {
    "status": "partial",
    "mutation": { "status": "ran", "score": 0.82, "killed": 9, "survived": 2, "timeout": 0, "unviable": 1 },
    "weakening": { "status": "ran", "findings": 0 },
    "tautology": { "status": "not_measured", "reason": "rust only in v1" },
    "per_test": { "status": "skipped" },
    "mix": { "status": "skipped" }
  }
}
```

| `test_value.status` | Meaning |
|---|---|
| `skipped` | All sub-signals off or not applicable (e.g. `mutation.mode=off` and no other engines enabled). |
| `partial` | At least one sub-signal ran or reported findings; others skipped or not measured. |
| `ran` | Every enabled sub-signal that applies to this pack finished (success or findings). |
| `not_measured` | Enabled but no tool or language support (same semantics as coverage when `cargo-llvm-cov` is missing). |

Human report (terminal and Markdown) adds a **Test value** section after **Mutation** when any sub-signal is not `skipped`:

```text
Test value
  Mutation (diff): 82% (9 killed, 2 survived, 0 timeout, 1 unviable) — 11 mutants in changed functions
  Weakening: none in diff
  Tautology: not measured (enable when implemented)
```

Findings use the existing `findings[]` array with `engine` set to `test_value` or the legacy `mutation` engine where noted below.

## Sub-signals

### 1. Mutation (diff-scoped) — deliverable 2

**Purpose:** Plant small defects in functions touched by the diff; count how many the test suite kills.

**Rust (v1):** `cargo mutants` via the existing `mutation` engine in `sc-engines`, extended to:

- Default to **diff-scoped** mutants: functions whose definitions intersect the git diff against `--diff <ref>` (or whole tree when no diff, subject to `max_mutants` and time budget).
- Report `killed`, `survived`, `timeout`, `unviable`, `mutation score` = killed / (killed + survived + timeout), and one `mutation.survivor` finding per surviving mutant with `file:line` and mutant description.
- Respect `[mutation] mode`, `max_mutants`, `budget_seconds` and the global analyze `--budget-seconds` pool (same as today).

**Relationship to today:** `mutation` in JSON stays for backward compatibility; `test_value.mutation` mirrors the same run for the new section. New UI reads `test_value` first; agents may read either during transition.

**Other packs (planned support matrix):**

| Pack | Mutation tool | Initial `test_value.mutation` |
|---|---|---|
| `rust` | `cargo mutants` | measured when installed |
| `node` | Stryker (project-dependent) | not measured until an adapter exists |
| `python` | mutmut / cosmic-ray | not measured |
| `go` | go-mutesting | not measured |
| `java` | PIT / Major | not measured |
| `csharp`, `php`, `cpp`, `bash`, `command` | none standardized in `sc` | not measured |

Missing tools produce `engine.unavailable` / `status: not_measured` with install hints; never exit 2 solely because mutation was skipped.

### 2. Test weakening in diff — deliverable 3

**Purpose:** Detect edits that make tests easier to pass in the same change set as product code.

**Heuristics (Rust first, diff-only):**

- Test function or module removed while non-test Rust in scope changed.
- Assertion loosening: `assert_eq!` → `assert!`, exact literal → substring/`contains`, wider numeric tolerance.
- `#[ignore]`, `#[should_panic]` added, or `expected` panic message removed.
- Snapshot or golden file deleted or regenerated without review marker (manual waiver only in v1 — report, do not auto-pass).

**Output:** `test_value.weakening` counts and findings `test_value.weakening` (severity warning, disposition `ask`) with diff hunk references.

### 3. Tautology checks — deliverable 4

**Purpose:** Flag tests that cannot fail meaningfully.

**Rust heuristics (initial):**

- `#[test]` with empty body or only `assert!(true)`.
- Expected value computed by calling the function under test (or a copy of its logic) before `assert_eq!`.
- Tests that mock the unit under test and only assert mock return values.

**Output:** `test_value.tautology` with per-file findings `test_value.tautologous_test` (warning, `ask`). Requires parsing `#[test]` bodies (syn or rustc span data); not run on whole repo in v1 — scope to tests touched in diff or tests covering changed symbols.

### 4. Per-test value — deliverable 5

**Purpose:** List tests that consume runtime but kill zero unique mutants.

**Method:** After a diff-scoped mutation run, attribute each killed mutant to the minimal set of tests that failed (from `cargo mutants` JSON or junit coupling where available). Combine with per-test duration from the `tests` engine.

**Output:** `test_value.per_test.low_value[]` entries `{ name, file, duration_ms, unique_mutants_killed }` sorted by cost.

### 5. Test type mix — deliverable 6

**Purpose:** Summarize unit vs integration vs e2e where detectable.

**Rust:** `tests/` and `#[test]` in `src/` → unit; `tests/*.rs` integration; `#[ignore]` e2e or `tests/e2e` convention if present.  
**Node:** `*.test.ts` vs `*.spec.ts` vs Playwright/Cypress paths (best-effort).  
Unknown bucket → `other`.

## Scoping, budget, and `--diff`

| Mode | Mutation scope | Weakening / tautology |
|---|---|---|
| Full tree | All mutants listed up to `max_mutants`, else skip with reason | All test files under scope |
| `--diff BASE` | Mutants in functions touched by `BASE...HEAD` | Hunks in `BASE...HEAD` only |
| Empty diff | Mutation `status: empty-diff` (existing behavior) | Weakening skipped |

Time limits:

1. Share the analyze run's remaining `--budget-seconds` with compile, test, coverage, and lint.
2. `[mutation].budget_seconds` caps mutation only (default 180).
3. Listing mutants (`cargo mutants --list`) uses a short sub-cap (30s today) before applying the rest to the run.

## Gates and `--fail-on` (future)

Until MB2090 approves gating:

- No new enforced gate by default.
- `mutation` gate stays **advisory** unless the user adds `mutation` to `--fail-on` or `gates.fail_on` (existing behavior).

After sign-off (separate PR):

- Optional `test_value` gate: fail when mutation score on diff falls below a threshold or when `test_value.weakening` findings exist.
- Thresholds live under `[test_value]` in `analyzer.toml` (to be specified in the implementation PR).

## Fixtures and proof

Each engine ships `testdata/` crates with:

- **Good:** a real test that kills planted mutants / has assertions.
- **Bad:** tautological or weakening fixture the engine must flag.

Proof in PRs: failing-first test output, then green after fix; `sc analyze` JSON excerpt showing `test_value` (report-only, verdict may still `PASS`).

## Out of scope

- Running browser e2e mutation or full-repo mutation on every CI push without a budget.
- Proving semantic equivalence of tests to specifications (LLM may comment in `spec` / `--llm`, not `test_value`).
- Blocking merge on agent-generated tests by default.
- Rewriting or deleting user tests automatically.
- Measuring DeepSWE-style "ban all tests" experiments inside `sc` (this design only supplies local signals).

## Implementation order

Driven by the [ranked shortlist](../research/testing-antipatterns.md#ranked-build-shortlist-drives-deliverables-26).

1. **Design + research** (leaf PR) — MB2090 review before behavior changes.
2. Diff-scoped mutation reporting under `test_value` (trunk, report-only) — **P0**.
3. Weakening detector: assertion loosening, test deletes, ignore/skip added in diff (trunk) — **P0/P1**.
4. Tautology: assertion-free and SUT-derived expected values (leaf or trunk) — **P1**.
5. Per-test value and mix (trunk) — **P1/P2**.
6. Sleep/timing lint, snapshot churn, mock tautologies, flake/order — future, mostly out of scope v1.

## References

- Existing mutation engine: `crates/sc-engines/src/mutation.rs`, `[mutation]` in `docs/reference/config.md`.
- Gate policy: `docs/reference/gates.md`, REVIEW_POLICY.md §6.12–6.13.
- Kun Chen DeepSWE eval (Oct 2026): context for MB2090 priority; not a normative requirement for `sc`.
