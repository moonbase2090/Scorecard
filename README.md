# Scorecard

[![CI](https://github.com/moonbase2090/Scorecard/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/moonbase2090/Scorecard/actions/workflows/ci.yml?query=branch%3Amain)
[![License: MPL 2.0](https://img.shields.io/badge/License-MPL%202.0-blue.svg)](LICENSE)
[![MSRV 1.85](https://img.shields.io/badge/MSRV-1.85-blue.svg)](https://github.com/moonbase2090/Scorecard/blob/main/Cargo.toml)

`sc` is a local code-quality gate. In one run it checks that a project builds, its tests pass, complex code is covered by tests ([CRAP](docs/crap.md)), no secrets are committed, the linter is clean, and every import is a declared dependency. It prints a verdict for people and a JSON scorecard for agents and CI. Ten language packs are built in. Project site: [scorecardcli.com](https://scorecardcli.com).

The secrets scan checks files up to 64 MiB even when they contain NUL bytes. It skips gitignored large files. A non-ignored file over 64 MiB produces `secrets.partial`.

The secrets gate recognizes Slack incoming webhooks, Stripe restricted live keys (`rk_live_`), and AWS provider secret keys in Terraform, alongside the existing token and key formats.

## Quickstart

Download `sc` for your platform and run it on a project:

```bash doctest network
VERSION=v0.1.5
case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) TARGET=aarch64-apple-darwin ;;
  Darwin-x86_64) TARGET=x86_64-apple-darwin ;;
  Linux-aarch64) TARGET=aarch64-unknown-linux-gnu ;;
  Linux-x86_64) TARGET=x86_64-unknown-linux-gnu ;;
esac
URL="https://github.com/moonbase2090/Scorecard/releases/download/$VERSION/sc-$VERSION-$TARGET.tar.gz"
SC_DIR="$(mktemp -d)"
trap 'rm -rf "$SC_DIR"' EXIT
HTTP_STATUS="$(curl -sSL -w '%{http_code}' -o "$SC_DIR/scorecard.tar.gz" "$URL")" || HTTP_STATUS=000
if [ "$HTTP_STATUS" = 200 ]; then
  tar -xzf "$SC_DIR/scorecard.tar.gz" -C "$SC_DIR" sc
elif [ "$HTTP_STATUS" = 404 ] && [ -n "${SCORECARD_DOCS_BINARY:-}" ]; then
  cp "$SCORECARD_DOCS_BINARY" "$SC_DIR/sc"
else
  printf 'Could not download Scorecard %s (HTTP %s). Try again or install from source: https://github.com/moonbase2090/Scorecard#install\n' "$VERSION" "$HTTP_STATUS" >&2
  exit 1
fi
"$SC_DIR/sc" analyze .
```

In a terminal the report looks like this (from `testdata/good_crate`):

```text
sc 0.1.5  testdata/good_crate  rust  5ac851c clean  scope tree

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
exit 0: enforced gates passed
```

Exit status: `1` a gate selected by `--fail-on` failed; `0` none did; `2` `sc` could not run. The report still shows `FAIL` when any enforced gate fails, even if `--fail-on` keeps the process exit code at 0. [Reading the report](docs/report.md) explains every section.

A dirty report lists up to three changed paths. JSON includes every path in `git.dirty_paths`. Scorecard's saved report, cache and coverage files under `.sc/`, and files selected by `--out` do not make a clean checkout dirty.

`sc analyze . --diff BASE` scores only what changed against that git ref. When the project is a subdirectory of a larger repository, the changed paths are still relative to the project (`src/a.rs`).

Scans honor nested `.gitignore` files and skip common build and vendor directories such as `build/`, `target/`, `dist/`, and `node_modules/`. See [scan scope configuration](docs/reference/config.md#scope) to add exclusions or include generated paths.

`sc analyze . --format sarif` writes SARIF for GitHub code scanning. Only `secrets.*` findings are level `error`, which code scanning counts as a security vulnerability. Every other rule, including a failing test or missing coverage, is level `warning` and still fails its gate ([CI](docs/how-to/ci.md)).

Clippy findings use the first error with a file, line, and lint name. If no such error exists, they use the first warning with those details:

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

The secrets scan also finds PEM private keys split across source string literals, including Go or Java concatenations and YAML lists. See [PEM examples](examples/README.md#pem-keys-in-source-files).

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

Release [v0.1.5](https://github.com/moonbase2090/Scorecard/releases/tag/v0.1.5). Each `.tar.gz` holds `sc`, `sc-mcp` (the MCP server), `LICENSE`, and `README.md`.

| Platform | Asset |
|---|---|
| macOS, Apple silicon | `sc-v0.1.5-aarch64-apple-darwin.tar.gz` |
| macOS, Intel | `sc-v0.1.5-x86_64-apple-darwin.tar.gz` |
| macOS installer | `sc-v0.1.5-macos.pkg`, or `sc-v0.1.5-universal-apple-darwin.dmg`. Installs `sc` and `sc-mcp` to `/usr/local/bin` |
| Linux, arm64 | `sc-v0.1.5-aarch64-unknown-linux-gnu.tar.gz` |
| Linux, x86_64 | `sc-v0.1.5-x86_64-unknown-linux-gnu.tar.gz` |

To check a download, fetch `SHA256SUMS` into the same directory and verify only the files you have:

```bash
curl -fsSLO https://github.com/moonbase2090/Scorecard/releases/download/v0.1.5/SHA256SUMS
shasum -a 256 -c --ignore-missing SHA256SUMS   # Linux: sha256sum -c --ignore-missing SHA256SUMS
```

From source (Rust 1.85 or newer), in a clone of this repository:

```bash
cargo install --locked --path crates/sc-cli
cargo install --locked --path crates/sc-mcp
```

Rust coverage needs `rustup component add llvm-tools` and `cargo install cargo-llvm-cov`. Without them `sc` still runs and reports coverage as not measured. Other packs need their own tools; see [packs](docs/packs.md).

For Python projects with a `src/` layout, `sc` prepends each source root to `PYTHONPATH` while tests and coverage run. Pytest then imports the checkout files that Scorecard measures, even when another package copy is installed.

For a Cargo workspace, analyze the root to check every member. Analyze a member directory to check only that package and avoid sibling crates.

## License

Scorecard is licensed under the [Mozilla Public License 2.0](LICENSE). You can use it in commercial products. If you distribute modified MPL-covered files, you must make their source available under MPL-2.0.
