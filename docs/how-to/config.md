# Configure sc

`sc` needs no config. To change a default, put `analyzer.toml` at the root of the project you analyze. Every key is in the [config reference](../reference/config.md).

## Common changes

```toml
# Two language markers in one root: pick one; automatic detection remains unchanged.
pack = "rust"

# Wall-clock budget for the run, in seconds. --budget-seconds overrides it.
# budget_seconds = 900

# For separate language-pack roots in a monorepo, use pack = "multi" instead.

[gates]
# Also fail on CRAP offenders and undeclared dependencies.
fail_on = ["types", "tests", "crap", "secrets", "lint", "sca"]
# Allow more complex, less tested functions.
crap_threshold = 40

[scope]
# Add project-specific paths to the built-in scan exclusions.
exclude = ["src/generated/**"]
# Reopen one built-in generated directory when you need to inspect it.
# Remove or negate a matching .gitignore rule too.
# include_generated = ["build/**"]

[engines]
# Run read-only Rust dependency and secrets scans alongside Cargo phases.
# Cargo commands remain sequential; this is off by default.
parallel = false

[commands]
# Lint command for the Rust and command packs. Empty skips lint.
lint = "cargo clippy --workspace --all-targets -- -D warnings"
# Extra Rust test arguments. Harness options follow `--`.
# test_args = ["--locked", "--features", "extra", "--", "--test-threads=1"]
```

At a workspace root, this checks every member. If you analyze a member directory, Scorecard removes the workspace scope so sibling crates do not affect its lint result.

`commands.test_args` is passed to `cargo test` and `cargo llvm-cov`. At a workspace root Scorecard still adds `--workspace`. Do not put `--workspace`, `--manifest-path`, or `--color` in the list. Put a test-name filter after `--`, with the harness options: `test_args = ["--", "extra_on"]`. It filters the full suite, and `cargo llvm-cov` receives it the same way. With `--diff`, Scorecard also adds each selected test name, so the run covers both.

Check that a config is read:

```bash doctest
cat > analyzer.toml <<'EOF'
[gates]
crap_threshold = 40
EOF
sc analyze . --format json | jq '.crap.threshold'
```

prints `40`. Unknown keys are ignored without a warning, so a misspelled key has no effect.

## One-off overrides

Flags win over the file for a single run:

```bash doctest
sc analyze . --fail-on types,tests >/dev/null
```

For settings shared by every project on this machine, use `~/.config/sc/analyzer.toml`. It is read only when the project has no `analyzer.toml`. `sc config path` prints that path. `sc config init` creates the directory and writes a starter from `analyzer.toml.example`. It does not replace an existing file. `sc config init --force` replaces it. The macOS package runs `sc config init` for the console user and does not overwrite.
