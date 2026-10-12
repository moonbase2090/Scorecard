# Testing antipatterns Scorecard could detect

Research backing for [test-value design](../design/test-value.md).  
Status: **draft** for MB2090 review (ships with design PR #220).

## How to read this document

Each antipattern includes: a one-line definition, a minimal bad example, why it wastes time or hides bugs (with citations where available), how `sc` could detect it, applicable language packs, false-positive risk, and typical runtime cost on a developer laptop or CI runner.

Evidence quality varies. Peer-reviewed empirical studies are cited with links. Practitioner catalogs (Meszaros, Google Testing Blog) are cited for definitions, not for quantitative claims unless they provide numbers. **Agent-era** risks are supported by recent preprints (2025–2026); treat effect sizes as directional until independently reproduced.

## Summary table

| Antipattern | Detection | Packs (v1) | FP risk | Cost | Build priority |
|---|---|---|---|---|---|
| Low mutation score on diff (incl. weak / mirror-implementation oracles) | Dynamic (cargo mutants) + static follow-ons | Rust first | Low–med | High | **P0** |
| Assertion weakened in diff | Git diff + AST | Rust, then others | Med | Low | **P0** |
| Test deleted with code change | Git diff | All | Med | Low | **P0** |
| Assertion-free / always-true test | Static AST | Rust first | Med | Low | **P1** |
| Expected value from SUT call | Static AST / dataflow | Rust first | Med–high | Med | **P1** |
| Mock-only / tautology mock | Static AST | Rust, Node | High | Low | **P2** |
| Snapshot churn without review | Git diff | Node, Rust | High | Low | **P2** |
| `#[ignore]` / skip added in diff | Git diff | Rust, pytest markers | Low | Low | **P1** |
| Sleeps / timing in test | Static + lint | Many | Med | Low | **P2** |
| Order-dependent test | Dynamic (shuffle) | Rust, Go | Med | High | **P3** |
| Flaky (erratic) test | Dynamic (re-runs) | All | Med | Very high | **P3** |
| High cost, zero unique mutants | Dynamic (mutation attribution) | Rust first | Low | High | **P1** (after P0 mutation) |

The **[ranked build shortlist](#ranked-build-shortlist-drives-deliverables-26)** below is normative for deliverable order. Agent-era “mirror the implementation” tests are primarily caught by rank 1 (mutation) and rank 4 (static tautology), not a separate engine.

---

## Catalogs and prior art

### Meszaros — xUnit Test Patterns

Gerard Meszaros groups **18 test smells** into code smells (visible in test source), behavior smells (compile/run time), and project smells (symptoms for managers). The catalog includes **Assertion Roulette**, **Obscure Test**, **Conditional Test Logic**, **Erratic Test**, **Fragile Test**, **Slow Tests**, and **Test Code Duplication**. Definitions and refactorings are on [xUnitPatterns.com](http://xunitpatterns.com/TestSmells.html); the book *xUnit Test Patterns* (Addison-Wesley, 2007) documents patterns and smells in Parts II–III.

Scorecard will not implement all 18 smells in v1. The design prioritizes smells that (a) correlate with false confidence in CI and (b) are detectable with bounded automation.

### tsDetect — automated test smells (Java)

[tsDetect](https://testsmells.org/) (Peruma et al., ESEC/FSE 2020, [DOI 10.1145/3368089.3417921](https://doi.org/10.1145/3368089.3417921)) detects **19 smell types** in JUnit tests using AST rules. On a manually annotated benchmark of 65 files, the authors report **~96% precision and ~97% recall** (F-score ~96.5%). The tool is Java-specific; Scorecard’s Rust/Node paths will reuse *ideas* (e.g. assertion counting, duplicate logic) rather than port tsDetect wholesale.

### Mutation testing as a value signal

- **Just et al. (FSE 2014):** On 357 real faults in five open-source programs, mutant detection correlates with real-fault detection even when controlling for coverage ([paper PDF](https://homes.cs.washington.edu/~mernst/pubs/mutation-effectiveness-fse2014.pdf), [ACM](https://dl.acm.org/doi/10.1145/2635868.2635929)).
- **Papadakis et al. (ICSE 2018):** At large scale, correlation with real faults can look weak when test-suite size is confounded; still, **raising mutation score improves fault detection** versus random suites of the same size ([DOI 10.1145/3180155.3180183](https://doi.org/10.1145/3180155.3180183)).
- **Just & Just (ASE 2020):** Reanalysis argues coverage and mutation goals can be compared fairly; hybrid coverage→mutation selection matches industrial practice ([PDF](https://homes.cs.washington.edu/~rjust/publ/mutants_faults_revisited_ase_2020.pdf)).

**Implication for Scorecard:** diff-scoped mutation score is the strongest *dynamic* signal that tests are not merely green. It is expensive; default to report-only and tight budgets.

### Flaky tests

- **Lam et al. (ICSE 2020):** Lifecycle study on Microsoft projects; **asynchronous calls** are the leading flake cause; claimed “fixes” often do not reduce flake rate ([Microsoft Research](https://www.microsoft.com/en-us/research/publication/a-study-on-the-lifecycle-of-flaky-tests/)).
- **Ziftci et al. (ICSME 2020):** Google root-cause localization for flakes reported **82% accuracy** on 428 projects ([Google Research](https://research.google/pubs/de-flake-your-tests-automatically-locating-root-causes-of-flaky-tests-in-code-at-google/)).
- **Luo et al. (ISSTA 2019):** Industrial study: few distinct flaky tests can cause a large share of failing builds ([ACM](https://dl.acm.org/doi/10.1145/3293882.3330570)).

**Implication:** flake *detection* needs repeated runs (costly). v1: document as out of scope for default CI; optional future engine with explicit budget.

### Agent-era and LLM-generated tests

Internal context (Kun Chen, Oct 2026 DeepSWE eval, not a published paper): banning agent-written tests slightly improved solve rate with less time/spend; blocking all test runs did not change solve rate on a subset. Diagnosis: tests restate agent intent; failures can be “fixed” in code or tests.

Published adjacent work (preprints — verify before citing numbers externally):

- **Code-before-test workflow** ([arXiv:2607.05139](https://arxiv.org/html/2607.05139v1)): tests generated after faulty code reduced fault-detection effectiveness vs independent generation (**14% vs 25%** in their setup).
- **Misguidance from buggy code** ([arXiv:2607.22883](https://arxiv.org/abs/2607.22883)): buggy implementations steer LLMs toward tests that assert wrong behavior; specification-based prompting mitigates.
- **VibeCheck** ([arXiv:2609.05978](https://arxiv.org/html/2609.05978v1)): runnable LLM tests often lack strong assertions and edge cases (“execution–adequacy gap”).

**Implication:** mutation on the diff plus static tautology/weakening checks target the failure mode “green tests, no oracle independence” without requiring LLM-specific hooks.

---

## Antipatterns (detailed)

### 1. Low mutation score on changed code

**Definition:** Mutants seeded in functions touched by the diff survive the test suite.

**Bad example:** Change `fn discount(x) -> x * 0.9` to `x * 0.8`; tests still pass because they only call with `x = 0`.

**Why it matters:** Coverage and pass/fail do not measure fault detection; mutation scores correlate with finding real faults when used with care (FSE 2014, ICSE 2018).

**Scorecard detection:** Run `cargo mutants` (Rust) on diff-scoped functions; report score, survivors with `file:line`. Other packs: not measured until an adapter exists.

**Packs:** Rust (`cargo mutants`); Node/Python/Go/Java — planned adapters.

**FP risk:** Low for “survivor” findings (real surviving mutant). Med for score thresholds (project-specific).

**Cost:** High (minutes on medium crates; budget-capped).

---

### 2. Assertion weakened in the same diff as production changes

**Definition:** A change loosens test expectations while modifying code under test.

**Bad example:** `assert_eq!(parse("3"), Ok(3))` → `assert!(parse("3").is_ok())` in the same PR as `parse` changes.

**Why it matters:** Classic way to silence failures without fixing bugs; related to **Fragile Test** refactors that delete checks instead of updating behavior (Meszaros).

**Scorecard detection:** Git diff hunks on test files + AST compare assertion forms (Rust: `syn`); flag hunk pairs where production files in scope also changed.

**Packs:** Rust first; Node/TS via AST; Python via `ast`.

**FP risk:** Medium (intentional relaxation for flaky tests).

**Cost:** Low.

---

### 3. Test removed while production code changes

**Definition:** Test functions or modules deleted in a diff that also edits non-test code.

**Bad example:** Delete `#[test] fn rejects_negative()` while changing validation logic.

**Why it matters:** Reduces safety net exactly when behavior changes; tsDetect and Meszaros treat missing tests as project-level smell **Developers Not Writing Tests** / shrinking suite.

**Scorecard detection:** Diff path classification (`tests/`, `#[test]`, `*_test.rs`) vs `src/`.

**Packs:** All (path heuristics).

**FP risk:** Medium (renames, consolidations).

**Cost:** Low.

---

### 4. Assertion-free or always-true test

**Definition:** Test runs code but never checks an outcome (or only `assert!(true)`).

**Bad example:**

```rust
#[test]
fn runs() {
    let _ = foo();
}
```

**Why it matters:** Inflates pass rate and coverage without oracle; common in generated tests (VibeCheck weak-assertion theme).

**Scorecard detection:** Static: count macro invocations (`assert*`, `expect`, `should_panic`) per test function; zero → finding.

**Packs:** Rust first; extend per pack assertion idioms.

**FP risk:** Medium (custom assertion helpers).

**Cost:** Low.

---

### 5. Expected value computed by calling the function under test

**Definition:** Test uses SUT output as expected value (tautological oracle).

**Bad example:**

```rust
let got = double(2);
assert_eq!(got, double(2));
```

**Why it matters:** Test cannot fail if `double` is wrong consistently; aligns with LLM “error propagation” and mirror-implementation tests.

**Scorecard detection:** Static dataflow: expected expression contains call to same symbol as exercised API (intra-test, conservative).

**Packs:** Rust first.

**FP risk:** High without conservative rules (property tests, golden vectors).

**Cost:** Medium (AST + light analysis).

---

### 6. Tautology mock / mock-only test

**Definition:** Mock returns the value the assertion checks; SUT never exercised meaningfully.

**Bad example:** `mock.return(42); assert_eq!(service.run(), 42)` with `run` stubbed to return mock.

**Why it matters:** Passes if production is empty; practitioner literature describes as change-detector / tautology mock ([Yeda summary of Fowler/Google testing guidance](https://yeda-ai.com/kb/ai-coding/mock-returning-exact-value-test) — not a formal study).

**Scorecard detection:** Static pattern match on mock setup + assert; optional dynamic check that SUT module is linked (pack-specific).

**Packs:** Rust (`mockall`), Node (jest/vitest) — high FP.

**FP risk:** High.

**Cost:** Low static; medium if combined with coverage of SUT lines.

---

### 7. Snapshot / golden file churn

**Definition:** Large snapshot or golden updates in diff without corresponding assertion tightening.

**Bad example:** Regenerate entire `*.snap` after behavior change with no review.

**Why it matters:** Regenerated goldens can encode bugs; industry practice is human review — automation should flag volume, not auto-fail.

**Scorecard detection:** Diff stats on `*.snap`, `__snapshots__`, `*.golden`; correlate with production diff.

**Packs:** Node (Jest), Rust (`insta`).

**FP risk:** High.

**Cost:** Low.

---

### 8. Ignore / skip added in diff

**Definition:** `#[ignore]`, `pytest.mark.skip`, `it.skip`, etc. added while fixing related code.

**Bad example:** Add `#[ignore]` to failing test instead of fixing product.

**Scorecard detection:** Diff hunk on ignore attributes/markers + production change in scope.

**Packs:** Rust, Python, JS.

**FP risk:** Low–medium.

**Cost:** Low.

---

### 9. Sleep / fixed timing in tests

**Definition:** Test waits wall-clock time instead of synchronizing on condition.

**Bad example:** `std::thread::sleep(Duration::from_secs(2))` in unit test.

**Why it matters:** Flake and slowness; async timing is top flake cause (Microsoft ICSE 2020).

**Scorecard detection:** Static lint for sleep calls in test paths; optional allowlist.

**Packs:** Many.

**FP risk:** Medium (integration tests).

**Cost:** Low.

---

### 10. Order-dependent test

**Definition:** Test outcome depends on run order or shared global state.

**Bad example:** Test B assumes DB seeded by test A without isolation.

**Why it matters:** Erratic failures; Meszaros **Erratic Test**.

**Scorecard detection:** Dynamic: `cargo test -- --test-threads=1` vs shuffled order (Rust experimental); compare failures. Expensive.

**Packs:** Rust, Go.

**FP risk:** Medium.

**Cost:** High (multiple runs).

---

### 11. Flaky (erratic) test

**Definition:** Same commit, same env, test sometimes passes and sometimes fails.

**Why it matters:** Misleads CI and agents (Google ISSTA 2019; Microsoft ICSE 2020).

**Scorecard detection:** Repeated runs with seed; not default in v1.

**Packs:** All.

**FP risk:** Medium (env drift).

**Cost:** Very high.

---

### 12. Costly test, zero unique mutants killed

**Definition:** Test consumes runtime but is the only failing test for no mutants (after attribution).

**Why it matters:** Targets “6% less time” style waste from low-value agent tests (internal DeepSWE context — directional).

**Scorecard detection:** Requires mutation run + per-test failure attribution (`cargo mutants` JSON).

**Packs:** Rust first.

**FP risk:** Low for “zero mutants” list.

**Cost:** High (same as mutation).

---

## Ranked build shortlist (drives deliverables 2–6)

| Rank | Capability | Maps to deliverable | Rationale |
|---|---|---|---|
| 1 | Diff-scoped mutation score + survivors | #2 Mutation engine | Strongest evidence link to fault detection; directly answers “would tests catch planted bugs in this change?” Catches many weak / mirror-implementation oracles that static rules miss. |
| 2 | Test weakening + deletes in diff | #3 Test weakening | Cheap, high signal for agent/human gaming CI |
| 3 | Ignore/skip added in diff | #3 (same engine) | Low cost, low FP |
| 4 | Assertion-free / tautology (static) | #4 Tautology checks | Cheap; targets LLM weak-oracle pattern |
| 5 | Per-test cost vs mutants killed | #5 Per-test value | Needs #2; lists “expensive useless” tests |
| 6 | Unit / integration / e2e mix | #6 Test type mix | Cheap metadata for report context |
| 7 | Sleep/timing lint in tests | Future leaf | Cheap follow-on |
| 8 | Snapshot churn flags | Future leaf | Review aid, not gate |
| 9 | Tautology mock (static) | Stretch in #4 | High FP; report-only with clear evidence |
| 10 | Order dependence / flake detection | Out of scope v1 | Cost vs value; revisit with flake budget |

Deliverable order in the task file remains valid; **#3** should explicitly include ignore/skip and test deletion; **#4** follows once weakening lands.

## References

1. Meszaros, G. *xUnit Test Patterns*. Addison-Wesley, 2007. Smell catalog: http://xunitpatterns.com/TestSmells.html  
2. Peruma, A. et al. “tsDetect: An Open Source Test Smells Detection Tool.” ESEC/FSE 2020. https://doi.org/10.1145/3368089.3417921  
3. Just, R. et al. “Are Mutants a Valid Substitute for Real Faults in Software Testing?” FSE 2014. https://doi.org/10.1145/2635868.2635929  
4. Papadakis, M. et al. “Are mutation scores correlated with real fault detection?” ICSE 2018. https://doi.org/10.1145/3180155.3180183  
5. Just, R. & Just, R. “Revisiting the Relationship Between Fault Detection, Test Adequacy Criteria, and Test Set Size.” ASE 2020. https://homes.cs.washington.edu/~rjust/publ/mutants_faults_revisited_ase_2020.pdf  
6. Lam, W. et al. “A Study on the Lifecycle of Flaky Tests.” ICSE 2020. https://www.microsoft.com/en-us/research/publication/a-study-on-the-lifecycle-of-flaky-tests/  
7. Ziftci, C. et al. “De-Flake Your Tests…” ICSME 2020. https://research.google/pubs/de-flake-your-tests-automatically-locating-root-causes-of-flaky-tests-in-code-at-google/  
8. Luo, Q. et al. “Root causing flaky tests in a large-scale industrial setting.” ISSTA 2019. https://dl.acm.org/doi/10.1145/3293882.3330570  
9. (Preprint) LLM code-before-test workflow. arXiv:2607.05139, 2026. https://arxiv.org/html/2607.05139v1  
10. (Preprint) VibeCheck LLM unit tests. arXiv:2609.05978, 2026. https://arxiv.org/html/2609.05978v1  
