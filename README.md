# scorecard

`sc` is a local-first code quality gate. It wraps Rust's own tools, computes cyclomatic complexity and CRAP, and prints a stable JSON scorecard that an agent can act on.

`sc` detects one language pack from the tree. `Cargo.toml` is Rust. `package.json` is Node. A Python manifest is Python. A top-level or `scripts/` or `bin/` shell file, with no other marker, is Bash. `go.mod` is Go. `pom.xml` or Gradle is Java. A root `.csproj` or `.sln` is C#. `composer.json` is PHP. `CMakeLists.txt` is C++. Two markers and no override is an error. Set `pack` in `analyzer.toml`, or pass `--pack`, to name the pack. `command` is only an override: it runs secrets plus a lint command you set yourself.

A gate with `enforced: false` is reported and does not fail the process. Rust enforces types, tests, CRAP, secrets, dependency checks, and lint. Python enforces `python3 -m compileall`, pytest when a test suite is present, Ruff, imports declared in `pyproject.toml`, secrets, and CRAP. Pytest writes line coverage when pytest-cov is available. The other packs use the same CRAP formula. Node reads c8, Java reads JaCoCo, C# reads Cobertura from `dotnet-coverage`, PHP reads Clover, Bash reads kcov, and C++ reads lcov after CTest. A missing report scores uncovered functions as coverage 0. Report paths are `.ca/coverage/coverage-final.json` (c8), `target/site/jacoco/jacoco.xml` (JaCoCo), `.ca/coverage/csharp.cobertura.xml` (dotnet-coverage), `.ca/coverage/clover.xml` (PHPUnit with pcov), `.ca/coverage/kcov` (kcov Cobertura), and `.ca/coverage/cpp.info` (lcov). The command pack has no coverage runner. Node checks syntax with `node --check` and runs `npm test` when a test script exists. Bash uses `bash -n`, and `shellcheck` or `bats` when they are installed. Go runs `go build`, `go test -coverprofile`, and `go vet`. C++ uses `g++ -fsyntax-only`. Java, C#, and PHP run their compilers when `javac`, `dotnet`, or `php` is on `PATH`. If the host binary is missing and the `scorecard-tools` image is present, the same command runs in that image. Build it locally with `docker build -t scorecard-tools:latest docker/scorecard-tools`. No registry is required. The image includes Rust (`cargo`, `clippy`, `llvm-tools`, `cargo-llvm-cov`), Python (`python3`, `pytest`, `pytest-cov`, `ruff`, `uv`), and Go 1.27.1, plus the other pack tools. A missing compiler and a missing image are reported and do not fail the process. `--diff` selects Rust `#[test]` names (`test_selection` is `rust-tests`). Every other pack uses the full suite.

On a Rust tree, `sc` runs `cargo check`, `cargo test`, complexity, `cargo llvm-cov`, CRAP, hallucinated imports, and a small secrets scan. `--diff` and `--paths` narrow the CRAP gate. Mutation, the spec check, and the LLM review are off unless you ask for them. `sc-mcp` serves four MCP tools. `--format sarif` writes SARIF 2.1.0.

## Install

```bash
cargo build --release -p ca-cli
# binary: target/release/sc
```

Or install it into Cargo's bin directory:

```bash
cargo install --path crates/ca-cli
```

Build with Rust 1.85 or newer (current crates use that MSRV). Coverage needs the LLVM tools component for the active toolchain, plus `cargo-llvm-cov`:

```bash
rustup component add llvm-tools
cargo install cargo-llvm-cov
```

On older toolchains the component is named `llvm-tools-preview`. If either tool is missing, `sc` still runs: function coverage is treated as 0, a `coverage.missing` warning is recorded, CRAP is still computed, and the process exits 2 only when a required gate (`types` or `tests`) cannot run.

## Example

```bash
sc analyze testdata/good_crate --format json
sc analyze testdata/crap_untested
sc analyze testdata/crap_tested --format md
```

```text
sc analyze [PATH] [--diff [BASE]] [--diff-head REV] [--paths FILE] [--spec PATH]
            [--format json|md|sarif|all] [--out PATH] [--fail-on LIST]
            [--mutation off|diff|full] [--llm off|on] [--intent TEXT]
            [--budget-seconds N] [--config PATH]
```

| Flag | Default |
|---|---|
| `PATH` | `.` |
| `--format` | `json` (`md`, `sarif`, or `all`) |
| `--fail-on` | `types,tests,crap,secrets,lint` |
| `--mutation` | `off` |
| `--llm` | `off` |
| `--intent` | none |
| `--budget-seconds` | `120` |
| `--config` | `analyzer.toml` in the crate, then `~/.config/ca/analyzer.toml` |

`--format all` prints JSON, then Markdown, on stdout. With `--out`, JSON, Markdown, and SARIF are written as sibling `.json`, `.md`, and `.sarif` files. `--format sarif` writes SARIF to stdout and to `--out`.

Exit codes:

| Code | Meaning |
|---|---|
| 0 | Configured gates passed |
| 1 | A configured gate failed |
| 2 | Analyzer error (missing crate, missing required toolchain, timeout on compile or tests) |

Skipping coverage, mutation, or the LLM does not by itself exit 2.

## CRAP

Cyclomatic complexity (not cognitive). For each function:

```text
CRAP(m) = CC(m)^2 * (1 - cov(m))^3 + CC(m)
```

`cov` is that function's line coverage from llvm-cov, in `0..1`. The default threshold is **30** (`gates.crap_threshold` in `analyzer.toml`). A score equal to the threshold passes; only a score above it fails the `crap` gate (`crap.over_threshold`).

Coverage needed to stay at or under 30:

| CC | cov needed |
|---|---|
| ≤5 | 0% |
| 10 | ~42% |
| 15 | ~57% |
| 20 | ~71% |
| 25 | ~80% |
| ≥31 | refactor; tests cannot save it |

Names from llvm-cov are matched to parsed functions on a best-effort basis (demangled debug names). A function with no coverage record is treated as uncovered. When that happens and coverage did run, the scorecard includes a `coverage.unmatched` warning.

At CC 5 and 0% coverage, CRAP is exactly 30, so the function passes. At CC 12 and 0% coverage, CRAP is 156.

Dimension scores start at 1.0. Each error finding subtracts 0.25 and each warning subtracts 0.05, floored at 0. Correctness takes compile and test findings; maintainability takes complexity, coverage, and CRAP; efficiency and security stay at 1.0 until those engines exist. See `crates/ca-core/src/score.rs`.

## Fixtures

| Path | What it checks |
|---|---|
| `testdata/failing_test` | M0. Failing unit test. Exit 1, verdict `fail`, finding `test.failed` on that test. |
| `testdata/good_crate` | Small crate that typechecks and passes tests. Exit 0. |
| `testdata/crap_untested` | CC-heavy `classify`, no tests. Exit 1, finding `crap.over_threshold`. |
| `testdata/crap_tested` | The same `classify` with tests that cover its branches. Exit 0 (CRAP under 30 when llvm-cov is installed). |
| `testdata/fake_dep` | Uses `missing_crate` under `cfg(any())`. Exit 0. Warning `sca.hallucinated_import`, disposition `ask`. |

`sc analyze testdata/crap_untested` should finish in well under 30 seconds after dependencies are already fetched. These fixtures have no crates.io dependencies.

In tree mode the scorecard fields `loc_changed`, `files_changed`, and `coverage_changed` describe the analyzed `src` tree, not a git diff. `hallucinated_imports` is 0. `mutation.status` is `skipped`.

`gates.new_fn_untested_cc` defaults to 15. In tree and `--paths` mode, every function in scope at or above that complexity with 0% coverage produces `complexity.untested` and fails the `crap` gate. With `--diff`, CRAP scores only changed functions, and `complexity.untested` scores only functions that are new relative to the base. At CC 11 and 0% coverage, CRAP already fails the default threshold, and `complexity.untested` does not fire.

## Config

See `analyzer.toml.example`. Copy it to `analyzer.toml` or `~/.config/ca/analyzer.toml`. `--fail-on` overrides `gates.fail_on`.

`secrets` matches a small token set (AWS access keys, GitHub tokens, Slack tokens, Stripe live keys, and private-key blocks). An undeclared dependency is a strongly advised warning (`sca.hallucinated_import`, disposition `ask`). It stays on the scorecard and does not fail the process. `std`, `core`, `alloc`, `crate`, `self`, and `super` are allowed. Python uses the same warning for an import that is not in `pyproject.toml`.

`--spec FILE` checks that paths named in the file exist and that `fn`, `struct`, `enum`, `trait`, `type`, and `const` names in the file are public items. Gaps fill `spec.gaps`. The `spec` gate fails when `--spec` is set or when `spec` is in `--fail-on` and the file is missing.

`--mutation diff` runs `cargo mutants --in-diff` against the same base as `--diff` (or `HEAD~1`). `--mutation full` runs the whole crate. Survivors are `mutation.survivor`. Timeouts are counted and are not kills. If `cargo-mutants` is missing, or the diff has more mutants than `mutation.max_mutants` (default 50), the engine is skipped with `engine.unavailable`. The mutation gate fails only when the mode is not `off`.

`--intent TEXT` is stored on the scorecard and sent with `--llm on`. It is the caller's goal. Scorecard does not read agent transcripts.

On `--diff`, `cargo test` runs only `#[test]` functions in files that mention a changed symbol, at most eight names. An empty set, or a larger set, runs the full `cargo test`. Each command is listed in `runs` with its exit code and duration. `cargo check`, coverage, and lint are recorded there too.

`commands.lint` defaults to `cargo clippy -- -D warnings`. A non-zero exit is `lint.failed` with disposition `fix`. A missing lint command is `engine.unavailable` with disposition `ask`. The `lint` gate fails the process only when `lint` is in `--fail-on`. Perf warnings stay disposition `ignore`.

`--llm on` sends the spec to an OpenAI-compatible `/chat/completions` endpoint. The built-in endpoint is `http://127.0.0.1:11434/v1`. If that endpoint is unchanged and `XAI_API_KEY` is set, the call uses SpaceXAI at `https://api.x.ai/v1` with model `grok-4.5`. The model may add `spec.llm_gap` warnings. It does not invent CRAP or mutation scores. A failed call skips the engine and does not fail the run. The loop allows 12 tool rounds and 2000 output tokens. Tools are `get_file`, `get_span`, `callers_of`, `tests_covering`, and `spec_section`.

`sc-mcp` speaks MCP over stdio. The tools are `analyze_paths`, `analyze_diff`, `explain`, and `list_findings`. `explain` and `list_findings` read `.ca/last-scorecard.json` from the last analyze. Parse results are cached in `.ca/cache/parse-v1.json`.

`sc setup` makes that server visible to agents on this computer. It writes the skill to `~/.grok/skills/scorecard`, `~/.claude/skills/scorecard`, `~/.cursor/skills/scorecard`, and `~/.agents/skills/scorecard`. It registers `sc-mcp` in `~/.grok/config.toml`, `~/.cursor/mcp.json`, and `~/.claude.json` when `sc-mcp` is on `PATH`. Run it again after you install or move the binary. Reload MCP servers in the agent after that.

`.github` is not required. `action/action.yml` installs `sc`, runs it, and uploads SARIF when the format is `sarif` or `all`.

Perf findings `perf.nested_loop` and `perf.clone_in_loop` are warnings. They do not have a gate.

## License

Scorecard is licensed under the [Mozilla Public License 2.0](LICENSE). You can use it in commercial products. If you distribute modified MPL-covered files, you must make their source available under MPL-2.0.
