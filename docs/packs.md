# Packs

`sc` detects one language pack from the tree.

| Marker | Pack |
|---|---|
| `Cargo.toml` | Rust |
| `package.json` | Node |
| a Python manifest | Python |
| a top-level, `scripts/`, or `bin/` shell file, and no other marker | Bash |
| `go.mod` | Go |
| `pom.xml` or Gradle | Java |
| a root `.csproj` or `.sln` | C# |
| `composer.json` | PHP |
| `CMakeLists.txt` | C++ |

Two markers and no override is an error. Set `pack` in `analyzer.toml`, or pass `--pack`, to name the pack. `command` is only an override: it runs secrets plus a lint command you set yourself.

A gate with `enforced: false` is reported and does not fail the process. Rust enforces types, tests, CRAP, secrets, dependency checks, and lint. Python enforces `python3 -m compileall`, pytest when a test suite is present, Ruff, imports declared in `pyproject.toml`, secrets, and CRAP. Pytest writes line coverage when pytest-cov is available. The other packs use the same CRAP formula.

| Pack | Coverage report |
|---|---|
| Node | `.sc/coverage/coverage-final.json` from c8 |
| Java | `target/site/jacoco/jacoco.xml` |
| C# | `.sc/coverage/csharp.cobertura.xml` |
| PHP | `.sc/coverage/clover.xml` from PHPUnit with pcov |
| Bash | `.sc/coverage/kcov` Cobertura |
| C++ | `.sc/coverage/cpp.info` from lcov after CTest |
| Go | `go test -coverprofile` |

The command pack has no coverage runner. A missing report scores uncovered functions as coverage 0.

Node checks syntax with `node --check` and runs `npm test` when a test script exists. Bash uses `bash -n`, and `shellcheck` or `bats` when they are installed. Go runs `go build`, `go test -coverprofile`, and `go vet`. C++ uses `g++ -fsyntax-only`. Java, C#, and PHP run their compilers when `javac`, `dotnet`, or `php` is on `PATH`. If the host binary is missing and the `scorecard-tools` image is present, the same command runs in that image. Build it locally with `docker build -t scorecard-tools:latest docker/scorecard-tools`. No registry is required. The image includes Rust (`cargo`, `clippy`, `llvm-tools`, `cargo-llvm-cov`), Python (`python3`, `pytest`, `pytest-cov`, `ruff`, `uv`), and Go 1.27.1, plus the other pack tools. A missing compiler and a missing image are reported and do not fail the process.

`--diff` selects Rust `#[test]` names (`test_selection` is `rust-tests`). Every other pack uses the full suite. On `--diff`, `cargo test` runs only `#[test]` functions in files that mention a changed symbol, at most eight names. An empty set, or a larger set, runs the full `cargo test`.

On a Rust tree, `sc` runs `cargo check`, `cargo test`, complexity, `cargo llvm-cov`, CRAP, hallucinated imports, and a small secrets scan. `--diff` and `--paths` narrow the CRAP gate. Mutation, the spec check, and the LLM review are off unless you ask for them.

## Flags

```text
sc analyze [PATH] [--diff [BASE]] [--diff-head REV] [--paths FILE] [--spec PATH]
            [--format json|md|sarif|html|all] [--out PATH] [--fail-on LIST]
            [--pack PACK] [--mutation off|diff|full] [--llm off|on] [--intent TEXT]
            [--budget-seconds N] [--config PATH]
```

| Flag | Default |
|---|---|
| `PATH` | `.` |
| `--format` | `json` (`md`, `sarif`, `html`, or `all`) |
| `--fail-on` | `types,tests,crap,secrets,lint` |
| `--pack` | detect one pack. `rust`, `node`, `python`, `bash`, `go`, `java`, `csharp`, `php`, `cpp`, or `command` |
| `--mutation` | `off` |
| `--llm` | `off` |
| `--intent` | none |
| `--budget-seconds` | `120` |
| `--config` | `analyzer.toml` in the tree, then `~/.config/sc/analyzer.toml` |

`--format all` prints JSON, then Markdown, on stdout. With `--out`, JSON, Markdown, SARIF, and HTML are written as sibling `.json`, `.md`, `.sarif`, and `.html` files. `--format sarif` writes SARIF to stdout and to `--out`. `--format html` writes a self-contained visual report (no network requests) to stdout and to `--out`.

| Exit | Meaning |
|---|---|
| 0 | Configured gates passed |
| 1 | A configured gate failed |
| 2 | Analyzer error (missing path, missing required toolchain, timeout on compile or tests) |

Skipping coverage, mutation, or the LLM does not by itself exit 2.

## Fixtures

| Path | What it checks |
|---|---|
| `testdata/failing_test` | Failing unit test. Exit 1, verdict `fail`, finding `test.failed` on that test. |
| `testdata/good_crate` | Small crate that typechecks and passes tests. Exit 0. |
| `testdata/crap_untested` | CC-heavy `classify`, no tests. Exit 1, finding `crap.over_threshold`. |
| `testdata/crap_tested` | The same `classify` with tests that cover its branches. Exit 0 when llvm-cov is installed. |
| `testdata/fake_dep` | Uses `missing_crate` under `cfg(any())`. Exit 0. Warning `sca.hallucinated_import`, disposition `ask`. |

`sc analyze testdata/crap_untested` should finish in well under 30 seconds after dependencies are already fetched. These fixtures have no crates.io dependencies.

In tree mode the scorecard fields `loc_changed`, `files_changed`, and `coverage_changed` describe the analyzed `src` tree, not a git diff. `hallucinated_imports` is 0. `mutation.status` is `skipped`.

Perf findings `perf.nested_loop` and `perf.clone_in_loop` are warnings. They do not have a gate.

`.github` is not required. `action/action.yml` installs `sc`, runs it, and uploads SARIF when the format is `sarif` or `all`.
