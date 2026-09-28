# Configure sc

`sc` needs no config. To change a default, put `analyzer.toml` at the root of the project you analyze. Every key is in the [config reference](../reference/config.md).

## Common changes

```toml
# Two language markers in one tree (for example Cargo.toml and package.json): pick one.
pack = "rust"

[gates]
# Also fail on undeclared dependencies.
fail_on = ["types", "tests", "crap", "secrets", "lint", "sca"]
# Allow more complex, less tested functions.
crap_threshold = 40

[scope]
# Leave generated or vendored code out of complexity and CRAP.
exclude = ["target/**", "generated/**", "vendor/**"]

[commands]
# Lint command for the Rust and command packs. Empty skips lint.
lint = "cargo clippy --all-targets -- -D warnings"
```

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

For settings shared by every project on this machine, use `~/.config/sc/analyzer.toml`. It is read only when the project has no `analyzer.toml`.
