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

Names from the coverage tool are matched to parsed functions on a best-effort basis. Only a function with a coverage record is scored. A function with no record gets no CRAP number and no `crap.over_threshold` or `complexity.untested` finding, so it cannot fail the gate itself. The scorecard reports it with a warning: `coverage.unmatched` in the Rust pack, which names the functions, or `coverage.missing` in the other packs. A measured function that fails still fails the gate. [Gates](reference/gates.md) says when the `crap` gate is advisory. Measured 0% coverage is an error.

Test code is not scored. Rust skips `#[test]` and `#[cfg(test)]` items. The other packs skip files under `test/`, `tests/`, `__tests__/`, `spec/`, `testdata/`, or a directory ending in `.Tests` or `.Test`. They also skip files named like tests: `test_*`, `*_test`, `*_tests`, `*_spec`, `*_unittest`, `*-test`, `*-spec`, `*.test.*`, `*.spec.*`, `*Test`, `*Tests`, and `conftest.py`.

At CC 5 and 0% coverage, CRAP is exactly 30, so the function passes. At CC 12 and 0% coverage, CRAP is 156.

`gates.new_fn_untested_cc` defaults to 15. In tree and `--paths` mode, every function in scope at or above that complexity with 0% coverage produces `complexity.untested` and fails the `crap` gate. With `--diff`, CRAP scores only changed functions, and `complexity.untested` scores only functions that are new relative to the base. At CC 11 and 0% coverage, CRAP already fails the default threshold, and `complexity.untested` does not fire.

Dimension scores are described in [Reading the report](report.md#terminal-layout).
