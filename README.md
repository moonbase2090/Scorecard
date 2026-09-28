# Scorecard

[![CI](https://github.com/moonbase2090/Scorecard/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/moonbase2090/Scorecard/actions/workflows/ci.yml?query=branch%3Amain)
[![License: MPL 2.0](https://img.shields.io/badge/License-MPL%202.0-blue.svg)](LICENSE)
[![MSRV 1.85](https://img.shields.io/badge/MSRV-1.85-blue.svg)](https://github.com/moonbase2090/Scorecard/blob/main/Cargo.toml)

`sc` is a local code-quality gate. In one run it checks that a project builds, its tests pass, complex code is covered by tests ([CRAP](docs/crap.md)), no secrets are committed, the linter is clean, and every import is a declared dependency. It prints a verdict for people and a JSON scorecard for agents and CI. Ten language packs are built in. Project site: [scorecardcli.com](https://scorecardcli.com).

## Quickstart

Download `sc` for your platform and run it on a project:

```bash doctest network
VERSION=v0.1.3
case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) TARGET=aarch64-apple-darwin ;;
  Darwin-x86_64) TARGET=x86_64-apple-darwin ;;
  Linux-aarch64) TARGET=aarch64-unknown-linux-gnu ;;
  Linux-x86_64) TARGET=x86_64-unknown-linux-gnu ;;
esac
curl -fsSL "https://github.com/moonbase2090/Scorecard/releases/download/$VERSION/sc-$VERSION-$TARGET.tar.gz" | tar -xz sc
./sc analyze .
```

In a terminal the report looks like this (from `testdata/good_crate`):

```text
sc 0.1.3  testdata/good_crate  rust  5ac851c clean  scope tree

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
duration: 0.7s
exit 0: gates passed
```

Exit status: `0` the enforced gates passed, `1` an enforced gate failed, `2` `sc` could not run. [Reading the report](docs/report.md) explains every section.

Clippy findings point to the first diagnostic's file, line, and lint name:

```bash doctest project=lint_diagnostic
set +e
output="$(sc analyze . --format pretty 2>&1)"
status=$?
set -e
printf '%s\n' "$output"
test "$status" -eq 1
grep -Eq 'src/lib.rs:[0-9]+  clippy::let_and_return' <<<"$output"
```

Next:

- Put `sc` on your `PATH`: `sudo mv sc /usr/local/bin/`, or see [Install](#install).
- Add `.sc/` to `.gitignore`. `sc` keeps its last report and caches there.
- Run `sc setup` so coding agents on this machine can use `sc` ([agents](docs/how-to/agents.md)).
- Write a user config with `sc config init`, or put `analyzer.toml` at the project root ([configure](docs/how-to/config.md)).

## Docs

| I want to | Read |
|---|---|
| Understand a report | [Reading the report](docs/report.md) |
| Fix a failing or skipped check | [Troubleshooting](docs/troubleshooting.md) |
| Gate pull requests in CI | [CI](docs/how-to/ci.md) |
| Check changes before each commit | [Pre-commit](docs/how-to/pre-commit.md) |
| Let Claude Code, Cursor, or another agent run `sc` | [Agents and MCP](docs/how-to/agents.md) |
| Add an LLM spec review (Ollama, OpenRouter, Cursor) | [LLM providers](docs/how-to/llm.md) |
| Change gates, thresholds, or excluded paths | [Configure](docs/how-to/config.md) |
| Look up a flag, config key, gate, or rule id | [CLI](docs/reference/cli.md), [config](docs/reference/config.md), [gates](docs/reference/gates.md), [rules](docs/reference/rules.md) |
| Know what runs for my language | [Packs](docs/packs.md) |
| Read common questions | [FAQ](docs/faq.md) |

## Install

Release [v0.1.3](https://github.com/moonbase2090/Scorecard/releases/tag/v0.1.3). Each `.tar.gz` holds `sc`, `sc-mcp` (the MCP server), `LICENSE`, and `README.md`.

| Platform | Asset |
|---|---|
| macOS, Apple silicon | `sc-v0.1.3-aarch64-apple-darwin.tar.gz` |
| macOS, Intel | `sc-v0.1.3-x86_64-apple-darwin.tar.gz` |
| macOS installer | `sc-v0.1.3-macos.pkg`, or `sc-v0.1.3-universal-apple-darwin.dmg`. Installs `sc` and `sc-mcp` to `/usr/local/bin` |
| Linux, arm64 | `sc-v0.1.3-aarch64-unknown-linux-gnu.tar.gz` |
| Linux, x86_64 | `sc-v0.1.3-x86_64-unknown-linux-gnu.tar.gz` |

To check a download, fetch `SHA256SUMS` into the same directory and verify only the files you have:

```bash
curl -fsSLO https://github.com/moonbase2090/Scorecard/releases/download/v0.1.3/SHA256SUMS
shasum -a 256 -c --ignore-missing SHA256SUMS   # Linux: sha256sum -c --ignore-missing SHA256SUMS
```

From source (Rust 1.85 or newer), in a clone of this repository:

```bash
cargo install --locked --path crates/sc-cli
cargo install --locked --path crates/sc-mcp
```

Rust coverage needs `rustup component add llvm-tools` and `cargo install cargo-llvm-cov`. Without them `sc` still runs and reports coverage as not measured. Other packs need their own tools; see [packs](docs/packs.md).

For a Cargo workspace, analyze the root to check every member. Analyze a member directory to check only that package and avoid sibling crates.

## License

Scorecard is licensed under the [Mozilla Public License 2.0](LICENSE). You can use it in commercial products. If you distribute modified MPL-covered files, you must make their source available under MPL-2.0.
