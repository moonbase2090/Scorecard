# Changelog

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- The Scorecard action builds from source when the latest-release lookup fails, instead of stopping the job.
- An `a11y` engine with its own gate and score. It checks HTML in the web pack and JSX or TSX in the node pack against WCAG 2.2 criteria. It is advisory unless `--fail-on` names `a11y` or config enforces it. Rules can be disabled by id. See `docs/a11y.md`.

### Fixed

- The HTML report summary grid keeps the git SHA inside its own cell. A long value wraps or clips instead of painting over the tests column.
- `sca.hallucinated_import` treats workspace member package names and their dependencies as declared. A path dep written with a hyphen matches the underscore name used in source.
- A report-only run (a passing verdict with failing gates, such as `--fail-on ""`) no longer reads as a clean pass anywhere. The HTML flow strip and page title say `report only`, the markdown verdict line adds the failing count, and the terminal shows a `REPORT ONLY` banner and `exit 0: no enforced gate failed`.
- The HTML flow strip's scope box counts paths (`tree · 42 paths`) instead of calling them skipped, and the markdown scope line no longer claims every tree is `src`.

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
