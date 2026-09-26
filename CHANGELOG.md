# Changelog

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

- Split the readme into `docs/packs.md`, `docs/config.md`, `docs/mcp.md`, and `docs/crap.md`.

## [0.1.0]

- `sc` analyzes one language pack and prints a JSON, Markdown, SARIF, or HTML scorecard.
- Rust enforces types, tests, CRAP, secrets, dependency checks, and lint. Other packs report a smaller enforced set and share the CRAP formula.
- `sc-mcp` serves `analyze_paths`, `analyze_diff`, `explain`, and `list_findings` over stdio.
- License is MPL-2.0. Nothing is published to crates.io yet.
