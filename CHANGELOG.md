# Changelog

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- `--llm on` runs from `--intent` alone when no `--spec` is given. A review adds an optional `llm` object (backend, model, rounds, verdict, and up to 10 notes) to the scorecard, shown in the terminal, markdown, and HTML. A skipped review says why. The field is omitted when llm is off.
- `sc config init` writes `~/.config/sc/analyzer.toml` from `analyzer.toml.example`, creating the directory, and does not replace an existing file unless `--force` is set. `sc config path` prints that path. The macOS package runs `sc config init` for the console user.
- The Scorecard action builds from source when the latest-release lookup fails, instead of stopping the job.
- An `a11y` engine with its own gate and score. It checks HTML in the web pack and JSX or TSX in the node pack against WCAG 2.2 criteria. It is advisory unless `--fail-on` names `a11y` or config enforces it. Rules can be disabled by id. See `docs/a11y.md`.
- `[llm] backend = "cursor"` runs `cursor-agent` in read-only ask mode. It is opt-in. The default remains local Ollama, which does not contact Cursor. The cursor backend sends the spec and the files the agent reads to Cursor.
- `[llm] backend = "openai-compatible"` sends the spec and tool-read file text to a configurable base URL. The default URL is OpenRouter. The API key is read from the environment variable named by `api_key_env` (default `OPENROUTER_API_KEY`) and is not stored in config. Local Ollama stays the default.
- `[llm] max_tool_rounds` defaults to 36. When the cap is reached, the model gets one no-tools turn that must return a spec-gap verdict, and a reply that is not JSON is requested once more.

### Fixed

- The secrets gate reports GitHub OAuth and app tokens (`gho_`, `ghu_`, `ghs_`, `ghr_`) and an AWS secret access key. The documented example secret is ignored, and a low-entropy string is not a key.
- A spec-gap reply that is valid JSON followed by a stray `}` still parses. The reader takes the first complete JSON value.
- A spec-gap reply with a `}` before its first `{` no longer aborts the run. An empty `tool_calls` array is treated as no tool call, and a JSON retry does not resend tool calls without their results.
- The readme, `docs/config.md`, and `analyzer.toml.example` say where `analyzer.toml` goes: the root of the analyzed directory (usually the repo root), then `~/.config/sc/analyzer.toml`, with no parent or sub-directory search.
- The HTML report summary grid keeps the git SHA inside its own cell. A long value wraps or clips instead of painting over the tests column.
- `sca.hallucinated_import` treats workspace member package names and their dependencies as declared. A path dep written with a hyphen matches the underscore name used in source.
- A report-only run (`--fail-on ""` with failing gates) no longer reads as a clean pass anywhere. The HTML flow strip and page title say `report only`, the markdown verdict line adds the failing count, and the terminal shows a `REPORT ONLY` banner and `exit 0: no enforced gate failed`. A run whose enforced gates pass while an advisory gate such as `sca` fails stays `PASS`, with a note such as `1 advisory gate failing`, instead of switching to `REPORT ONLY`.
- The HTML flow strip's scope box counts paths (`tree · 42 paths`) instead of calling them skipped, and the markdown scope line no longer claims every tree is `src`.
- Reports no longer show unmeasured coverage as 0%. When the coverage engine did not run, the HTML coverage tile reads `coverage not measured`, CRAP rows say `not measured` (`--` in the terminal), and the HTML, markdown, and terminal CRAP tables note that the numbers assume 0% coverage. Measured coverage now shows its percentage next to the bar.
- A Python import of a local module or package is not a finding. Resolution uses the importing file's directory, `conftest.py` above that file, `src/`, `tests/`, `test/`, and pytest `pythonpath` (`pyproject.toml`, `pytest.ini`, `tox.ini`, or `setup.cfg`).
- An installed package, or a name on the package index, that is missing from `pyproject.toml` is `sca.undeclared_dependency` (advisory). `sca.hallucinated_import` is only a name that is not local, not installed, and absent from the index. If the index cannot be reached, the finding is `sca.import_unresolved` and is not included in `metrics.hallucinated_imports`. Finding text says Advisory, matching the gate. A Rust crate missing from `Cargo.toml` is `sca.undeclared_dependency`. The HTML undeclared-deps tile reads `metrics.undeclared_dependencies`. A hallucinated-imports tile appears only when that count is greater than zero.
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
- Gates outside `--fail-on` render as reported-only across JSON, Markdown, HTML, and the terminal.
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
