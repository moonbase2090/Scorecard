# Scorecard

[![CI](https://github.com/moonbase2090/Scorecard/actions/workflows/ci.yml/badge.svg?branch=develop)](https://github.com/moonbase2090/Scorecard/actions/workflows/ci.yml?query=branch%3Adevelop)
[![License: MPL 2.0](https://img.shields.io/badge/License-MPL%202.0-blue.svg)](LICENSE)
[![MSRV 1.85](https://img.shields.io/badge/MSRV-1.85-blue.svg)](https://github.com/moonbase2090/Scorecard/blob/develop/Cargo.toml)

The CI badge is workflow `ci` on `develop`. `ci.yml` is on `main` as well. License is MPL-2.0. The MSRV is Rust 1.85. The crates are not published, so there is no crates.io badge.

`sc` is a local code-quality gate. It picks one language pack, runs that pack's tools, and prints a scorecard an agent can act on.

## Quickstart

```bash
cargo install --path crates/sc-cli
cargo install --path crates/sc-mcp
sc analyze .
```

Exit 0 means the configured gates passed. Exit 1 means a gate failed. Exit 2 means the analyzer itself could not run.

## Sample

This is the output of `sc analyze testdata/good_crate --format md` on commit `0afc676`:

```markdown
# scorecard

**Verdict:** pass

**Repo:** testdata/good_crate

**Git:** 0afc676039d07842e5bfeb3c57a09df26e5c9a23 (clean)

**Scope:** tree of `src`. `loc_changed`, `files_changed`, and `coverage_changed` describe that tree.

**Engines run:** compile, tests, coverage, complexity, crap, sca, secrets, perf, lint

**Engines skipped:** spec, mutation, llm

## Gates

| Gate | Result | Reason |
|---|---|---|
| types | pass |  |
| tests | pass |  |
| crap | pass |  |
| sca | pass |  |
| secrets | pass |  |
| lint | pass |  |

## Scores

- correctness: 1.00
- efficiency: 1.00
- maintainability: 1.00
- security: 1.00

## Worst CRAP

Threshold 30.

| CRAP | CC | Coverage | Symbol | File |
|---|---|---|---|---|
| 1 | 1 | 100% | add | src/lib.rs |

## Findings

None.
```

The Git line is the Scorecard checkout that contained the fixture, because `testdata/good_crate` is not its own repository.

On a terminal, `sc analyze` prints the same scorecard as plain text. `--format pretty` forces that layout. A pipe stays JSON. This is `sc analyze testdata/good_crate --format pretty` on commit `2f0d4d9`, with color off:

```text
sc 0.1.0  testdata/good_crate  rust  2f0d4d9 clean  scope tree

PASS

gates
  [ok]  types            enforced
  [ok]  tests            enforced
  [ok]  crap             enforced
  [ok]  sca              advisory
  [ok]  secrets          enforced
  [ok]  lint             enforced

scores
  correctness      1.00  [##########]
  efficiency       1.00  [##########]
  maintainability  1.00  [##########]
  security         1.00  [##########]

worst crap  threshold 30
  CRAP   CC   COV  SYMBOL            LOCATION
     1    1  100%  add               src/lib.rs

findings
  (none)

engines run: compile, tests, coverage, complexity, crap, sca, secrets, perf, lint
engines skipped: spec, mutation, llm
duration: 0.8s
exit 0: gates passed
```

Colored captures of that command and of `testdata/failing_test` are in `examples/terminal/`.

## Install

Rust 1.85 or newer.

```bash
cargo build --release -p sc-cli -p sc-mcp
```

The binaries are `target/release/sc` and `target/release/sc-mcp`. `cargo install --path crates/sc-cli` and `cargo install --path crates/sc-mcp` put them on `PATH`.

Rust coverage also needs:

```bash
rustup component add llvm-tools
cargo install cargo-llvm-cov
```

On older toolchains the component is named `llvm-tools-preview`. If either tool is missing, `sc` still runs. Function coverage is treated as 0, a `coverage.missing` warning is recorded, and CRAP is still computed. The process exits 2 only when a required gate (`types` or `tests`) cannot run.

## GitHub Action

Run the gate in CI with the composite action (plain `bash` steps, no
runner-specific features):

```yaml
- uses: moonbase2090/Scorecard/action@develop
  with:
    fail-on: types,tests,crap,secrets,lint
    format: sarif
```

| Input      | Default                         | Notes                                    |
| ---------- | ------------------------------- | ---------------------------------------- |
| `spec`     | `""`                            | Path to a spec or task file              |
| `fail-on`  | `types,tests,crap,secrets,lint` | Comma-separated gates                    |
| `mutation` | `off`                           | `off`, `diff`, or `full`                 |
| `format`   | `sarif`                         | `json`, `md`, `sarif`, `html`, or `all`  |
| `diff`     | `""`                            | Git base ref; empty skips `--diff`       |

With `format: sarif` (or `all`) the SARIF report is uploaded via the
pinned `upload-sarif` step, so findings show up under code scanning.
Until the first tagged release, pin the action to `@develop` or a full
commit SHA.

## More

- [Examples](examples/README.md)
- [Packs](docs/packs.md)
- [Config](docs/config.md)
- [MCP](docs/mcp.md)
- [CRAP](docs/crap.md)
- [v0.1.0 readiness](docs/v0.1.0-readiness.md)

## License

Scorecard is licensed under the [Mozilla Public License 2.0](LICENSE). You can use it in commercial products. If you distribute modified MPL-covered files, you must make their source available under MPL-2.0.
