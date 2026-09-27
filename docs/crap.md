# CRAP

Cyclomatic complexity, not cognitive. For each function:

```text
CRAP(m) = CC(m)^2 * (1 - cov(m))^3 + CC(m)
```

`cov` is that function's line coverage, in `0..1`. The default threshold is 30 (`gates.crap_threshold` in `analyzer.toml`). A score equal to the threshold passes. Only a score above it fails the `crap` gate (`crap.over_threshold`). This repository's `analyzer.toml` keeps that default.

Coverage needed to stay at or under 30:

| CC | cov needed |
|---|---|
| ≤5 | 0% |
| 10 | ~42% |
| 15 | ~57% |
| 20 | ~71% |
| 25 | ~80% |
| ≥31 | refactor; tests cannot save it |

Names from the coverage tool are matched to parsed functions on a best-effort basis. A function with no coverage record is treated as uncovered. When that happens and coverage did run, the scorecard includes a `coverage.unmatched` warning. The CRAP number always assumes 0% for unmeasured functions, but when coverage was not measured for a function, `crap.over_threshold` and `complexity.untested` are warnings with a "coverage not measured" note instead of errors. Measured 0% coverage stays an error.

At CC 5 and 0% coverage, CRAP is exactly 30, so the function passes. At CC 12 and 0% coverage, CRAP is 156.

`gates.new_fn_untested_cc` defaults to 15. In tree and `--paths` mode, every function in scope at or above that complexity with 0% coverage produces `complexity.untested` and fails the `crap` gate. With `--diff`, CRAP scores only changed functions, and `complexity.untested` scores only functions that are new relative to the base. At CC 11 and 0% coverage, CRAP already fails the default threshold, and `complexity.untested` does not fire.

Dimension scores start at 1.0. Each error finding subtracts 0.25 and each warning subtracts 0.05, floored at 0. Correctness takes compile and test findings. Maintainability takes complexity, coverage, and CRAP. Efficiency and security stay at 1.0 until those engines exist. See `crates/sc-core/src/score.rs`.
