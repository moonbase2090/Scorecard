# Scorecard

[![CI](https://github.com/moonbase2090/Scorecard/actions/workflows/ci.yml/badge.svg)](https://github.com/moonbase2090/Scorecard/actions/workflows/ci.yml)
[![License: MPL 2.0](https://img.shields.io/badge/License-MPL%202.0-blue.svg)](LICENSE)
[![MSRV 1.85](https://img.shields.io/badge/MSRV-1.85-blue.svg)](https://github.com/moonbase2090/Scorecard)

The CI badge tracks `.github/workflows/ci.yml` (`name: ci`). It shows a status after that workflow is on the default branch.

`sc` is a local code-quality gate. It picks one language pack, runs that pack's tools, and prints a scorecard an agent can act on.

## Quickstart

```bash
cargo install --path crates/sc-cli
cargo install --path crates/sc-mcp
sc analyze .
```

Exit 0 means the configured gates passed. Exit 1 means a gate failed. Exit 2 means the analyzer itself could not run.

## Sample

This is the output of `sc analyze testdata/good_crate --format md` on commit `8afb726`:

```markdown
# scorecard

**Verdict:** pass

**Repo:** testdata/good_crate

**Git:** 8afb726b5c5438959f4273cbbbdaa902c02301cc (clean)

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

## More

- [Examples](examples/README.md)
- [Packs](docs/packs.md)
- [Config](docs/config.md)
- [MCP](docs/mcp.md)
- [CRAP](docs/crap.md)

## License

Scorecard is licensed under the [Mozilla Public License 2.0](LICENSE). You can use it in commercial products. If you distribute modified MPL-covered files, you must make their source available under MPL-2.0.
