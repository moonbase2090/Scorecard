# Changelog

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

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
