# Contributing to Scorecard

## Flow

`main` and `develop` are protected. Land changes like this:

1. Branch off `develop` with one of the `feature/`, `fix/`, `chore/`,
   `docs/`, `release/`, or `hotfix/` prefixes (branch rules reject the
   rest): `git checkout -b feature/<name> develop`.
2. Open a pull request **into `develop`**, not `main`.
3. `develop` merges to `main` at release time; pushing a `vX.Y.Z` tag builds
   the release assets (see `.github/workflows/release.yml`).

Keep pull requests small and focused. One commit per logical change.

Every PR update runs `check-ubuntu` (fmt, clippy, the full fixture test
suite), `deny`, and `msrv`. The macOS core tests, the LLVM coverage job,
and the self-analysis dogfood run on pushes to `develop`, so a green PR
still gets checked again at merge time. The macOS library tests require `cargo-llvm-cov`, the Rust `llvm-tools-preview` component, and `ruff`; CI installs them before `test core`.

## Before you push

All four must pass from the repo root:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
./target/release/sc analyze . --budget-seconds 600   # build sc first: cargo build --release -p sc-cli
```

The full instrumented suite is slower than the default 120s wall-clock
budget allows, so the self-analysis needs the raised budget (CI uses the
same value in the dogfood job).

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

## Development setup

Enable the repository's formatting hook:

```sh
sh scripts/setup-hooks.sh
```

The hook checks a temporary copy of the Git index. Staged Rust changes trigger
`cargo fmt --all -- --check` for the staged workspace. Staged JavaScript,
TypeScript, and JSON files trigger Biome 2.6.2 formatting checks. Install Rust
with rustfmt and Node.js with npm; the first web check downloads Biome through
`npx`. Working files and the index remain unchanged.

If the hook fails, format the reported files, stage the result with `git add`,
and retry. See [the pre-commit guide](docs/how-to/pre-commit.md) for commands.
Run `python3 scripts/test-pre-commit.py` to verify partial staging and web checks.
