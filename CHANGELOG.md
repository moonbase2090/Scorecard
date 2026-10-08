# Changelog

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- Add `sc skills install` and once-per-version automatic skill installation for detected agents (#183).
- Add contributor and review guidance against tautological tests (#195).

### Changed

- Show analysis progress steps in interactive sessions (#187).
- Print a short summary for `--out` on terminals while preserving full report output for pipes (#188).
- Raise the Rust MSRV to 1.88 and update the `ignore` dependency (#182, #190).
- Extract internal clippy, Makefile, HTML report, and scorecard data handling into focused modules without changing the JSON contract (#192, #197–#199).

### Fixed

- `test.failed` findings no longer use absolute panic paths outside the project (`/rustc/...`, the cargo registry, or other off-tree locations). The report points at the failing test's source file when it can be found, keeps the external location in the message and evidence, and never emits an absolute `file` in JSON or SARIF (#207).
- Skip CDK-generated `cdk.out` directories during source discovery (#189).
- Empty or blank `--fail-on` and `gates.fail_on` selections now fail with a configuration error. Use `none` by itself to return exit 0 for every gate. JSON and SARIF record the selected gates, and stderr names enforced failures omitted from the selection.
- A bare `--diff` (AUTO base) no longer falls back to `HEAD` in checkouts with no resolvable history. Shallow or single-commit checkouts now exit 2 with a message naming the fix (`fetch-depth: 0` or `--diff BASE`) instead of silently scoring zero paths. An explicit `--diff HEAD` keeps working.
- `--diff` skips an AUTO candidate (`HEAD~1`, `main`, or `master`) that is the same commit as `HEAD`, so a depth-1 clone of `main` or `master` exits 2 instead of scoring zero paths. A missing explicit base exits 2 with verdict `fail` and one stderr line that names the ref and `fetch-depth: 0`, instead of a raw git error and verdict `pass` (#203, #205).
- The secrets scan uses `git ls-files` in a work tree so tracked files under `vendor/`, `dist/`, `build/`, and other walker-skipped directories are still checked. Ignored paths and `[scope] exclude` behave as before (#202).
- The secrets scan skips files that begin with ELF, Mach-O, PE, WebAssembly, or `ar` archive magics instead of lossy-scanning release binaries (for example `sc` and `sc-mcp`), including when the file is over the 64 MiB text scan cap. Reports `secrets.skipped_object` (info). NUL-only prefixes still do not skip (#155).

## [0.1.6] - 2026-09-30

### Fixed

- GitHub Actions SARIF uploads now use an action compatible with Node 24, keeping code scanning reports uploadable on current runners.

## [0.1.5] - 2026-09-30

### Added

- Scans skip common build and dependency folders, plus source files marked `@generated` or `Code generated ... DO NOT EDIT`. Use `scope.include_generated` to inspect selected generated paths; reports identify generated paths that were included and suggest excluding them.

## [0.1.4] - 2026-09-29

### Added

- `--llm on` runs from `--intent` alone when no `--spec` is given. A review adds an optional `llm` object (backend, model, rounds, verdict, and up to 10 notes) to the scorecard, shown in the terminal, markdown, and HTML. A skipped review says why. The field is omitted when llm is off.
- `sc config init` writes `~/.config/sc/analyzer.toml` from `analyzer.toml.example`, creating the directory, and does not replace an existing file unless `--force` is set. `sc config path` prints that path. The macOS package runs `sc config init` for the console user.
- Task-based docs: a 60-second quickstart in the readme, how-tos (CI, pre-commit, agents and MCP, LLM providers, config), a reference for every flag, config key, gate, and rule id, a guide to reading the report, troubleshooting, and an FAQ. `scripts/check-docs.py` runs every doc example against the built CLI and checks the reference is complete; the `docs` workflow runs it on every pull request.
- A product quality bar in `REVIEW_POLICY.md` and the pull request template.
- The Scorecard action builds from source when the latest-release lookup fails, instead of stopping the job.
- An `a11y` engine with its own gate and score. It checks HTML in the web pack and JSX or TSX in the node pack against WCAG 2.2 criteria. It is advisory unless `--fail-on` names `a11y` or config enforces it. Rules can be disabled by id. See `docs/a11y.md`.
- `[llm] backend = "cursor"` runs `cursor-agent` in read-only ask mode. It is opt-in. The default remains local Ollama, which does not contact Cursor. The cursor backend sends the spec and the files the agent reads to Cursor.
- `[llm] backend = "openai-compatible"` sends the spec and tool-read file text to a configurable base URL. The default URL is OpenRouter. The API key is read from the environment variable named by `api_key_env` (default `OPENROUTER_API_KEY`) and is not stored in config. Local Ollama stays the default.
- `[llm] max_tool_rounds` defaults to 36. When the cap is reached, the model gets one no-tools turn that must return a spec-gap verdict, and a reply that is not JSON is requested once more.

### Fixed

- Enforced gate failures remain visible as a `FAIL` verdict in JSON, Markdown, HTML, SARIF, and terminal output when `--fail-on` suppresses exit 1. SARIF now includes the scorecard verdict; `--fail-on` selects the process exit code without hiding failures from gates that remain enforced.
- Python coverage now runs against the checkout's `src/` files when another package copy is installed. `sc` prepends each `src` root to `PYTHONPATH` for pytest and the `coverage.py` fallback, so tests and coverage use the same files.
- The secrets gate recognizes Slack incoming webhook URLs, Stripe restricted live keys (`rk_live_`), and AWS provider `secret_key` values in Terraform. Existing placeholder and entropy filtering applies to the detected values.
- Git status ignores Scorecard's saved report and generated cache and coverage files, plus the report files produced by `--out`. Other changes remain dirty, and the report names changed paths.
- `sc analyze --diff` scores a project that lives in a subdirectory of its git repository. Changed paths are relative to that project (`src/a.rs`), so the complexity and CRAP gates see the change. Before, the diff was empty and those gates passed without scoring it.
- Only `secrets.*` findings upload to code scanning at SARIF error level. Every other rule (`test.failed`, `coverage.missing`, lint, CRAP) uploads at warning, so a failing test or missing coverage report never counts as a security vulnerability. Finding severity in the JSON report is unchanged.
- A tracked file over 1 MiB that starts with NUL bytes is scanned for secrets instead of being skipped as binary. A secret in that file is reported; gitignored build output is still skipped.
- A PEM private key split across source string lines or YAML list items is reported as `secrets.private_key`. The scan handles Go and Java concatenation, backticks, Python string prefixes, triple-quoted lines, and quoted or bare YAML items.
- Opt-in Rust performance hints (`engines.perf = true`) flag a nested loop and a `.clone()` inside a loop as `perf.*` findings (disposition `ignore`). Paths under `tests/` or `benches/`, `src/test.rs` / `src/tests.rs`, inline `#[cfg(test)]` / `#[…::test]` items, files loaded by an out-of-line `#[cfg(test)] mod name;` (including on `--diff`, via a whole-tree lookup), and children under that module's directory, are not scanned. The parse cache stores unfiltered hits and applies the test-only filter on read, so flipping `cfg(test)` to a product module re-flags the file without waiting for it to change. The default stays off, so ordinary Rust does not change the report or the efficiency score.
- A finding with disposition `ignore` stays in the JSON report and is left out of the SARIF upload. A `perf.*` rule is disposition `ignore`. With the default `engines.perf = false`, a normal run's report and SARIF stay unchanged.
- When `analyzer.toml` names a Rust channel, check, test, coverage, lint, and mutation use it. A `rust-toolchain.toml` / `rust-toolchain` at or above the project clears an inherited `RUSTUP_TOOLCHAIN` so rustup reads the file; with no pin, the inherited variable is left alone.
- On a `--diff` run (pull-request reports), the scorecard names the changed paths and adds a one-line count of the other source paths still in the tree. Terminal, markdown, and HTML show the same count.
- When the test command ran and failed, `coverage.missing` says coverage was skipped and names the exit code. A missing tool or absent test script does not claim tests failed.
- Python dependency checks read `requirements.txt`, `setup.cfg`, and `setup.py` `install_requires`. A docstring is not an import. `_typeshed`, `import setuptools` in `setup.py`, and an import inside `try` / `except ImportError` are not findings.
- When tests run through `uv` and `uv` is not installed, the tests finding says `uv is not installed` instead of `pytest is not installed`.
- A nested loop, and a `.clone()` that the loop collects, are not findings by default. Efficiency is not reduced for that ordinary Rust unless `engines.perf` is on.
- A Rust import is declared only by that crate's `Cargo.toml`, plus `[workspace.dependencies]`. A dependency of another crate in the same workspace is still `sca.undeclared_dependency`.
- The Rust dependency check no longer reports crates that are already provided or declared. `proc_macro` is a builtin, like `std`, `core`, and `alloc`. A bare `use Name;` or `use Name as Alias;` of a name the file already imports by path, or defines, is a re-export and not a crate. A bare `use serde;` is still a crate. A header such as `[target.'cfg(windows)'.dependencies.windows-sys]` or `[dependencies.bytes]` declares that crate. A `use` of a module declared in the same crate counts as local, including a `mod` declared inside a macro call. The parse cache version changed, so the first run after upgrading re-parses every file.
- Tests cover the LLM review outcome, the PyPI name lookup, the Cursor review, and the markdown renderer. Those functions no longer exceed the CRAP threshold.
- A coverage report left by an earlier run is no longer scored as this run's coverage. Reports under `.sc/coverage`, including Python's `pytest.json`, are removed before the tests run. A report dated before the run started, or in the future, is ignored. That covers Jacoco's report under `target/` or `build/`. Before, when the coverage tool did not run, for example because c8 or Maven was not installed, the old report was read, coverage was listed as run, and a function whose test had been removed could keep its old coverage and pass the `crap` gate.
- A C or C++ tree with a `Makefile` or `configure` script no longer treats every compile error as advisory. A missing header or file stays advisory without `CMakeLists.txt` or `compile_commands.json`. An undeclared identifier, a type error, or a syntax error fails the types gate. Every file is checked, so a missing header does not hide a later error. `-I`, `-D`, and `-U` from the Makefile's `CFLAGS`, `CPPFLAGS`, and `CXXFLAGS` are passed to the check in that order, including `:=` and `?=`, and a quoted value such as `-DVERSION=\"1.0\"`. A later `-U` cancels an earlier `-D` of the same macro. `CFLAGS` is passed before `CPPFLAGS`, and `CXXFLAGS` before `CPPFLAGS`, matching `make`'s `COMPILE.c` and `COMPILE.cc`. A later assignment replaces the earlier flags. An escaped space in `-DMSG=\"hello\ world\"` stays one argument. A reference such as `$(DEFS)` is expanded by `make`, and an `ifeq` branch `make` does not take is not passed. When `make` cannot evaluate a variable or a conditional, the types check stays advisory. `make` is not run when the Makefile includes another file, uses `$(shell)`, `$(eval)`, or `$(file)`, or assigns with `!=`, so the check does not rewrite the tree. A `-D` or `-I` only in a recipe, a recipe variable other than `CFLAGS`, `CPPFLAGS`, or `CXXFLAGS`, a target-specific flag, or a `Makefile` in a subdirectory stays advisory. The default target is not built. A bare `#include` fails the gate.
- Node and Python score a file the tests never load at 0% coverage. c8 runs with `--all`, and pytest-cov gets each directory that holds a scored function as a `--cov` source, including a `src/` layout and namespace packages. Before, such a file had no coverage record, so its functions were not scored and a complex one could not fail the `crap` gate.
- One function with no coverage record no longer makes the whole CRAP gate advisory. A measured function over the CRAP threshold, or at or above `gates.new_fn_untested_cc` with 0% coverage, still fails the gate. The function with no record is not scored and does not clear that failure. When no measured function fails, the gate stays advisory.
- CRAP no longer scores test code in the non-Rust packs. Files under `test/`, `tests/`, `__tests__/`, `spec/`, `testdata/`, or a `.Tests` project, and files named like tests (`test_*.py`, `*_test.go`, `*.test.js`, `*.spec.ts`, `*Test.java`, `conftest.py`), are skipped. A CC 6 test helper at 0% coverage no longer fails the gate. Test files that the coverage tool leaves out no longer make the CRAP gate advisory. Rust already skipped `#[test]` and `#[cfg(test)]` code.
- Coverage is credited only to the file that was measured. `src/lib.rs` does not take hits from `helper/src/lib.rs`, and two `index.js` files do not share a basename. A JaCoCo row joins the package name to the source file. Report paths are matched once, so hits from another file are not added in. `vendor/`, `dist/`, and dot-directories can own those hits, and files deeper than six directories are included. CRAP does not score `vendor/`, `dist/`, or a dot-directory, so an unscored file there does not make the gate advisory. Each report file is matched once, and only against files with the same basename, so a large `vendor/` tree does not make the join slow.
- Pack detection treats a `Makefile` or `configure` plus a `.c`, `.cc`, `.cpp`, or `.cxx` file as C++, including when a shell script is also present. Headers do not select C++ and do not outvote another language. `Pipfile` is Python. A `.csproj` or `.sln` below the root is C#. When several manifests match, the run stays ambiguous unless only one of them has source files. A nested `.csproj` or `.sln` next to another language's sources exits 2 until `pack` is set in `analyzer.toml` or `--pack` is passed. C files are checked with `cc`, not `g++`. The types gate is enforced when `CMakeLists.txt` or `compile_commands.json` is present. Without them, only a missing header or file is advisory. A `package.json` next to only shell scripts stays Bash. The unknown-pack message names `--pack` and the `pack` key.
- The secrets scan is one walk for every pack. It honors `[scope] exclude` and reads every regular file, including `id_rsa`, `.npmrc`, `.envrc`, Terraform, and CI config such as `.circleci`. It skips dependency and cache directories (`.git`, `.venv`, `venv`, `node_modules`, `.tox`, `.mypy_cache`, `.pytest_cache`, `.next`, `.nuxt`, `.cache`, `.gradle`, `.yarn/cache`, `.sc`) and skips `target` and `dist` only at the project root. Directory symlinks are not followed, and each real file is read once. It no longer stops at depth 4 or 200 files. A non-UTF-8 byte no longer hides the rest of that file. A file that cannot be read fails the gate. A text file over 1 MiB, such as `package-lock.json` or `Cargo.lock`, is still scanned, so a lockfile with no secret does not fail the gate. A file over 1 MiB is scanned (lossy UTF-8) unless git ignores it; NUL bytes do not skip it. Over 64 MiB is still `secrets.partial`. A directory that cannot be listed fails the gate. The partial-scan record uses the 64 MiB limit. A gitignored file over that size is skipped and does not make the scan partial. A smaller file is still scanned, and one NUL in a comment does not hide a secret. In the secrets walk, `target/**` and `generated/**` match only the root directory, so `src/target` is still scanned. Complexity and CRAP still exclude `src/generated` and `src/target`.
- Node lint auto-detection treats `eslint.config.cjs` as an ESLint config file.
- Java pack Gradle projects now run `gradle test` even when the build file does not mention Jacoco. When Jacoco is configured, Scorecard still runs `jacocoTestReport` after tests.
- The Node types gate typechecks every `.ts` and `.tsx` file, including files tsconfig `include` skips, and runs `node --check` on `.js`, `.mjs`, and `.cjs` files. When `tsconfig.json` is present, `tsc --noEmit` uses a config that extends it and lists those files, because current `tsc` will not load `tsconfig.json` if files are passed on the command line.
- `--budget-seconds` is one wall clock for the run. A pack command that is still running at the deadline is killed with its child processes, and the report records the timeout instead of waiting until those children close the output pipes.
- At a Cargo workspace root, the default Rust lint command is `cargo clippy --workspace`; Cargo checks, tests, and coverage also include every member. Analyzing a member directory keeps those commands scoped to that package. A clippy warning no longer fails the lint gate. A repository that already denies warnings still fails, and `commands.lint = "cargo clippy --workspace -- -D warnings"` opts in at the workspace root.
- The secrets gate reports temporary AWS access key ids (`ASIA`) the same way as long-lived ones (`AKIA`).
- The secrets gate reports GitHub OAuth and app tokens (`gho_`, `ghu_`, `ghs_`, `ghr_`) and an AWS secret access key. The documented example secret is ignored, and a low-entropy string is not a key.
- The secrets gate ignores documentation placeholders and test tokens: AWS access key ids ending in `EXAMPLE`, PEM labels without key material, low-entropy or sequential `ghp_` / Slack / Stripe bodies, and tokens whose only content is the filler word `placeholder` or `example` (a mixed body that embeds those words is still reported).
- The secrets gate flags a normal multi-line PEM private key (BEGIN header, base64 body, END). A line that only names the PEM label, with no key material, is not a finding.
- A pytest suite under `test/`, a root `test_*.py` or `*_test.py` file, or a pytest config (`pytest.ini`, `[tool:pytest]` in `setup.cfg`, `[pytest]` in `tox.ini`) runs. A tree with no suite says so, instead of claiming the pack does not provide tests.
- A spec-gap reply that is valid JSON followed by a stray `}` still parses. The reader takes the first complete JSON value.
- A spec-gap reply with a `}` before its first `{` no longer aborts the run. An empty `tool_calls` array is treated as no tool call, and a JSON retry does not resend tool calls without their results.
- The readme, `docs/config.md`, and `analyzer.toml.example` say where `analyzer.toml` goes: the root of the analyzed directory (usually the repo root), then `~/.config/sc/analyzer.toml`, with no parent or sub-directory search.
- The HTML report summary grid keeps the git SHA inside its own cell. A long value wraps or clips instead of painting over the tests column.
- `sca.hallucinated_import` treats workspace member package names and their dependencies as declared. A path dep written with a hyphen matches the underscore name used in source.
- When no gate is enforced and advisory gates fail, reports say `REPORT ONLY` with the failing count. A run whose enforced gates pass while an advisory gate such as `sca` fails stays `PASS`, with a note such as `1 advisory gate failing`.
- The HTML flow strip's scope box counts paths (`tree · 42 paths`) instead of calling them skipped, and the markdown scope line no longer claims every tree is `src`.
- Incomplete coverage leaves unmatched functions unscored. Measured rows still show their coverage percent and can fail the gate. When nothing measured fails, the `crap` gate is advisory because some functions have no coverage record.
- Python now retries coverage collection with `coverage.py` when `pytest --cov` did not write a report, using the same `--source` directories as pytest-cov. Node coverage can come from either c8 or nyc; both run with `--all` so unloaded files stay measured.
- A Python import of a local module or package is not a finding. Resolution uses the importing file's directory, `conftest.py` above that file, `src/`, `tests/`, `test/`, and pytest `pythonpath` (`pyproject.toml`, `pytest.ini`, `tox.ini`, or `setup.cfg`).
- An installed package, or a name on the package index, that is missing from `pyproject.toml` is `sca.undeclared_dependency` (advisory). `sca.hallucinated_import` is only a name that is not local, not installed, and absent from the index. If the index cannot be reached, the finding is `sca.import_unresolved` and is not included in `metrics.hallucinated_imports`. Finding text says Advisory, matching the gate. A Rust crate missing from `Cargo.toml` is `sca.undeclared_dependency`. The HTML undeclared-deps tile reads `metrics.undeclared_dependencies`. A hallucinated-imports tile appears only when that count is greater than zero.
- Advisory `sca.hallucinated_import` findings now reduce security score proportionally (0.01 each) instead of using the full warning penalty.
- The HTML metric tile for `sca` misses reads `undeclared deps` instead of `hallucinated imports`. The JSON field is unchanged.
- Findings that list functions in `evidence.functions`, such as `coverage.unmatched`, name them in the HTML and markdown reports (up to 20, then a count), so an advisory CRAP gate says which function had no coverage record.
- Findings that carry raw command output in `evidence.log` show it behind a `full output` disclosure in the HTML report and a `<details>` block in markdown, below the one-line message.
- A `--diff` scorecard records its resolved base as `scope.base`, and the HTML, markdown and terminal reports say the CRAP count covers only the diff and point to the base branch's latest push run for the tree-wide count (or to a tree-scope run for a commit-ish base such as `HEAD~1`). No tree total is estimated.
- The HTML report groups findings by rule. Groups with errors come first, smallest first, so one test failure is not buried under CRAP cards. Small error groups start open and the rest collapse behind their counts. Each group shows at most 50 cards, and `complexity.untested` folds into the matching `crap.over_threshold` card. Accessibility findings are no longer listed twice. Repo-level findings drop the bare `.` location, and long commands in the runs table no longer push the exit and duration columns off the card.

## [0.1.3] - 2026-09-27

Version 0.1.2 was tagged but never published. This release includes its planned changes and the installer fix.

### Added

- The Scorecard action installs the prebuilt release for the runner and uploads an HTML report when the format is `html` or `all`.
- Document the HTML report, including the flow strip, in the readme and in `docs/html-report.md`.
- The macOS disk image ships `INSTALL.txt` and a signed `Install Scorecard.pkg` that installs `sc` and `sc-mcp` to `/usr/local/bin`. The same package is a release asset named `sc-v<version>-macos.pkg`.
- The `web` pack detects static HTML projects without a manifest, checks markup and internal links, and runs CRAP and secrets analysis.

### Fixed

- HTML report truncates a long git SHA to 12 characters so the header column does not overflow.
- Enforced failures outside `--fail-on` remain visible in the report verdict; the selected list controls the process exit code.
- Unprovided gates show a skipped pill and no longer count in the HTML flow-strip ratio.
- Git dirty detection snapshots at analyze start so Scorecard's own outputs do not mark a clean checkout dirty.
- The `sca` gate stays advisory when it passes, so JSON, Markdown, HTML, and the terminal layout agree. A failing dependency check still does not change the exit code.
- Local Rust modules no longer trigger undeclared dependency warnings.
- CRAP findings warn when coverage was not measured. Measured 0% coverage remains an error.
- Installer packaging reapplies the keychain partition list after importing the PKCS#12 certificate. A five-minute timeout stops signed `productbuild` from hanging on a keychain ACL prompt.

## [0.1.0] - 2026-09-26

Workspace version `0.1.0`. Not tagged. Not published to crates.io.

### Added

- `sc analyze --format pretty` prints a terminal scorecard. A terminal uses it when `--format` is omitted. A pipe stays JSON.
- `sc` analyzes one project tree and prints a JSON, Markdown, SARIF, or HTML scorecard. `--format all` writes that set. Exit 0 is a pass, exit 1 is a failed gate, and exit 2 is an analyzer error.
- Packs: Rust, Node, Python, Bash, Go, Java, C#, PHP, and C++. `--pack` names one of `rust`, `node`, `python`, `bash`, `go`, `java`, `csharp`, `php`, `cpp`, or `command` when several manifests match. `command` runs secrets plus a lint command you set.
- Rust enforces types, tests, CRAP, secrets, and lint. Undeclared dependencies are advisory and do not fail the process. Other packs report a smaller enforced set and use the same CRAP formula. The default threshold is 30.
- `sc-mcp` serves `analyze_paths`, `analyze_diff`, `explain`, and `list_findings` over stdio.
- `--mutation diff` or `--mutation full` runs `cargo-mutants` from `sc-engines`. Mutation stays off unless you ask.
- `--llm on` calls an OpenAI-compatible endpoint. The default is `http://127.0.0.1:11434/v1`. If that endpoint is unchanged and `XAI_API_KEY` is set, the call uses SpaceXAI at `https://api.x.ai/v1` with model `grok-4.5`.
- Docs for packs, config, MCP, and CRAP. The readme sample is `sc analyze testdata/good_crate --format md`.
- License is MPL-2.0. MSRV is Rust 1.85.

### Fixed

- Cargo workspaces are scored from each member's `src`, not only a top-level `src`.
- The private-key check no longer flags the line that defines the PEM markers.
- Coverage names match a generic function and a trait method to the syn symbol.
- Dogfood uses the default CRAP threshold of 30 again. `mutation::execute` is tested with a fake cargo runner, and the other functions that were over 30 are covered or split except where pull request #21 already split `analyze_rust`.
