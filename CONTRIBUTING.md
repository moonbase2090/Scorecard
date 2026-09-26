# Contributing to Scorecard

## Flow

`main` and `develop` are protected. Land changes like this:

1. Branch off `develop`: `git checkout -b feature/<name> develop`.
2. Open a pull request **into `develop`**, not `main`.
3. `develop` merges to `main` at release time; pushing a `vX.Y.Z` tag builds
   the release assets (see `.github/workflows/release.yml`).

Keep pull requests small and focused. One commit per logical change.

## Before you push

All four must pass from the repo root:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
./target/release/sc analyze .   # build sc first: cargo build --release -p sc-cli
```

## Commits

- Use a short imperative subject line (e.g. `Add SARIF output for findings`).
- Sign as `MB2090 <322824348+mb2090@users.noreply.github.com>`.
- No `Co-authored-by` or other attribution trailers.

## Issues

Bugs and feature requests use the templates in `.github/ISSUE_TEMPLATE/`.
Security issues are **not** handled here, see `SECURITY.md`.

## Test fixtures

`testdata/` holds `ghp_AAAA…` placeholder tokens for the secrets-gate
tests. They are not real credentials; secret scanning skips them
(see `.github/secret_scanning.yml`). Never replace them with live secrets.
