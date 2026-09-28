# Packs

`sc` detects one language pack from the tree.

| Marker | Pack |
|---|---|
| `Cargo.toml` | Rust |
| `package.json` with JavaScript or TypeScript in the tree | Node |
| a Python manifest (`pyproject.toml`, `requirements.txt`, `setup.py`, or `Pipfile`) | Python |
| a top-level, `scripts/`, or `bin/` shell file, and no other marker | Bash |
| `go.mod` | Go |
| `pom.xml` or Gradle | Java |
| a `.csproj` or `.sln` anywhere in the tree | C# |
| `composer.json` | PHP |
| `CMakeLists.txt`, or a `Makefile` / `configure` / `configure.ac` together with a `.c`, `.cc`, `.cpp`, or `.cxx` file. Headers do not select C++ | C++ |
| `index.html` or another root `.html` file, and no manifest | Web |

When several markers match, the run stays ambiguous unless only one of those languages has source files. A larger file count does not choose a pack. Set `pack` in `analyzer.toml`, or pass `--pack`. A `package.json` beside only shell scripts is Bash, not Node. A shell script does not hide C or C++ sources. Headers do not select C++ and do not force a pack on a Python project. `command` is only an override: it runs secrets plus a lint command you set yourself. A root HTML file does not override a manifest such as `package.json`. Pass `--pack web` when a manifest is present and the tree is still a static site.

The web pack needs no external tools. It parses HTML with `html5ever`, checks internal `href` and `src` paths against the tree, and scores CRAP on `.js` files and inline scripts. Secrets use the same patterns as the other packs. The `html` gate is enforced when `fail_on` is the built-in list or names `html`. Set `[html] enforce = "off"` to report markup without failing the process, or `"on"` to enforce it on a custom `fail_on` list. The `links` gate is advisory unless `fail_on` names `links` or `[links] enforce = true`. Accessibility checks are the `a11y` engine. See `docs/a11y.md`. The gate is advisory unless `fail_on` names `a11y` or `[a11y] enforce = true`.

A gate with `enforced: false` is reported and does not fail the process. Rust enforces types, tests, CRAP, secrets, and lint. An undeclared dependency is advisory (`sca`) and does not change the exit code. Python enforces `python3 -m compileall`, pytest when a suite is present (`test/`, `tests/`, a root `test_*.py` or `*_test.py`, or a pytest config), Ruff, secrets, and CRAP. A local Python module is not a finding. Imports are read from code, not from docstrings or comments. `_typeshed`, `import setuptools` in `setup.py`, and an import inside `try` / `except ImportError` are not findings. A dependency named in `pyproject.toml`, `requirements.txt`, `setup.cfg`, or `setup.py` `install_requires` is declared. An installed or published import missing from those files is the same advisory (`sca.undeclared_dependency`). `sca.hallucinated_import` is only a name that resolves nowhere. Python first tries `pytest --cov`, then falls back to `coverage.py` when needed. The other packs use the same CRAP formula.

| Pack | Coverage report |
|---|---|
| Node | `.sc/coverage/coverage-final.json` from c8 or nyc |
| Java | `target/site/jacoco/jacoco.xml` |
| C# | `.sc/coverage/csharp.cobertura.xml` |
| PHP | `.sc/coverage/clover.xml` from PHPUnit with pcov |
| Bash | `.sc/coverage/kcov` Cobertura |
| C++ | `.sc/coverage/cpp.info` from lcov after CTest |
| Go | `go test -coverprofile` |

Node runs c8 or nyc with `--all`. Python passes each directory that holds a scored function to pytest-cov as a `--cov` source, and the `coverage.py` fallback uses the same directories as `--source`. In both packs, a file the tests never load is scored at 0% coverage instead of being left out.

The command pack has no coverage runner. When a pack writes no coverage report, no function is scored: the CRAP table is empty, and the `crap` gate is advisory with the reason that coverage was not measured. The run reports `coverage.missing`.

Node checks JavaScript with `node --check`. When a `.ts` or `.tsx` file is present and `tsc` is on `PATH`, those files are typechecked even if tsconfig `include` skips them: with a `tsconfig.json`, `tsc --noEmit` uses a small config that extends it and lists every TypeScript file; without one, the files are passed on the command line. `node --check` still runs on `.js`, `.mjs`, and `.cjs` files. `npm test` runs when a test script exists. Bash uses `bash -n`, and `shellcheck` or `bats` when they are installed. Go runs `go build`, `go test -coverprofile`, and `go vet`. C files are checked with `cc -fsyntax-only -x c`. C++ files are checked with `g++ -fsyntax-only`. An undeclared identifier, a type error, a syntax error, or a bare `#include` fails the types gate. Every file is checked. `-I` and `-D` from the Makefile's `CFLAGS`, `CPPFLAGS`, and `CXXFLAGS` are passed through, including `:=`, `?=`, and a later assignment that replaces the earlier flags. An escaped space stays in the same argument. Without `CMakeLists.txt` or `compile_commands.json`, the check is advisory only when every error is a missing header or file. Java, C#, and PHP run their compilers when `javac`, `dotnet`, or `php` is on `PATH`. If the host binary is missing and the `scorecard-tools` image is present, the same command runs in that image. Build it locally with `docker build -t scorecard-tools:latest docker/scorecard-tools`. No registry is required. The image includes Rust (`cargo`, `clippy`, `llvm-tools`, `cargo-llvm-cov`), Python (`python3`, `pytest`, `pytest-cov`, `ruff`, `uv`), and Go 1.27.1, plus the other pack tools. A missing compiler and a missing image are reported and do not fail the process.

`--diff` selects Rust `#[test]` names (`test_selection` is `rust-tests`). Every other pack uses the full suite. At a workspace root, `--diff` runs matching tests with `cargo test --workspace`, at most eight names; an empty set or a larger set runs the full workspace suite. When analyzing one workspace member, Cargo tests stay scoped to that package.

At a Cargo workspace root, `sc` runs `cargo check --workspace`, `cargo test --workspace`, `cargo llvm-cov --workspace`, and `cargo clippy --workspace`. When analyzing one member, those commands run only for that package, so failures in siblings do not fail the run. A standalone package is its own workspace root. `--diff` and `--paths` narrow the CRAP gate. Mutation, the spec check, and the LLM review are off unless you ask for them. A Cargo workspace is scored from each member's `src` directory, found with `cargo metadata`. A top-level `src` is included when it exists.

Flags, output formats, and exit status are in the [CLI reference](reference/cli.md).

## Fixtures

| Path | What it checks |
|---|---|
| `testdata/failing_test` | Failing unit test. Exit 1, verdict `fail`, finding `test.failed` on that test. |
| `testdata/good_crate` | Small crate that typechecks and passes tests. Exit 0. |
| `testdata/workspace_src` | Virtual Cargo workspace with no top-level `src`. Exit 0. Scope includes both members, and CRAP is not zero. |
| `testdata/crap_untested` | CC-heavy `classify`, no tests. Exit 1, finding `crap.over_threshold`. |
| `testdata/crap_tested` | The same `classify` with tests that cover its branches. Exit 0 when llvm-cov is installed. |
| `testdata/fake_dep` | Uses `missing_crate` under `cfg(any())`. Exit 0. Warning `sca.undeclared_dependency`, disposition `ask`. |
| `testdata/local_mod` | `pub use` of a local `mod`. Exit 0. No dependency finding. |
| `testdata/py_local_import` | Imports a root module, root `conftest.py`, and a module on pytest `pythonpath`. Exit 0. No dependency finding. |
| `testdata/web_site` | Static HTML. Exit 0. Pack `web`. |
| `testdata/web_site_bad` | Missing doctype and viewport, a misnested tag, a missing local link, and a token. Exit 1. |

`sc analyze testdata/crap_untested` should finish in well under 30 seconds after dependencies are already fetched. These fixtures have no crates.io dependencies.

In tree mode the scorecard fields `loc_changed`, `files_changed`, and `coverage_changed` describe the analyzed `src` tree, not a git diff. `hallucinated_imports` and `undeclared_dependencies` are 0. `mutation.status` is `skipped`. In `--diff` mode `scope.base` records the resolved base ref (omitted in other modes), and `crap_over_threshold` counts only changed functions.

A nested loop, and a `.clone()` that the loop collects, are not findings.

`.github` is not required. `action/action.yml` installs `sc`, runs it, and uploads SARIF when the format is `sarif` or `all`.
