# Config reference

`sc` works without a config file. To change a default, create `analyzer.toml` at the root of the project you analyze. [analyzer.toml.example](../../analyzer.toml.example) lists every key with its default.

## Where sc looks

1. `--config PATH`, if given.
2. `analyzer.toml` in the analyzed directory (the `PATH` given to `sc analyze`). Parent and sub-directories are not searched.
3. `~/.config/sc/analyzer.toml`.

Only the first file found is read. Command-line flags override the matching keys. Unknown keys are ignored without a warning, so check spelling against the tables below.

## Keys

| Key | Default | Meaning |
|---|---|---|
| `install_agent_skills` | `true` | Allow installer hooks to install the Scorecard skill for detected agents. Hooks read this from the user config `~/.config/sc/analyzer.toml`. Set it to `false` to opt out. `SCORECARD_NO_AGENT_SKILLS=1` also skips automatic installation. |
| `pack` | detected | Language pack when the tree has more than one marker. Same values as `--pack`. |
| `toolchain` | empty | Rustup channel for check, test, coverage, and lint (`1.85.0`, `stable`, …). When set, Cargo uses that channel. When empty, a `rust-toolchain.toml` / `rust-toolchain` at or above the project clears an inherited `RUSTUP_TOOLCHAIN` so rustup reads the file; with no file, the inherited variable is left alone. |

### `[gates]`

| Key | Default | Meaning |
|---|---|---|
| `gates.fail_on` | `["types", "tests", "crap", "secrets", "lint"]` | [Gates](gates.md) whose failures set exit 1. `--fail-on` overrides; already enforced failures remain in the report verdict. An unknown gate name is an error (exit 2). |
| `gates.crap_threshold` | `30` | A function with a [CRAP](../crap.md) score above this fails the `crap` gate. |
| `gates.new_fn_untested_cc` | `15` | A function at or above this complexity with 0% measured coverage fails the `crap` gate (`complexity.untested`). With `--diff`, only new functions count. |

### `[scope]`

| Key | Default | Meaning |
|---|---|---|
| `scope.exclude` | `[]` | Additional paths to leave out of scans. A matching exclusion wins over `scope.include_generated`. |
| `scope.include_generated` | `[]` | Paths that may reopen built-in generated or vendored directory skips, or generated file markers. `.gitignore` rules still apply. |

All recursive scans respect `.gitignore`, including nested files. They also skip `.git`, `.hg`, `.svn`, `.sc`, and other dot-directories. The built-in generated and vendored directories are `target`, `dist`, `build`, `out`, `cdk.out`, `coverage`, `generated`, `vendor`, `node_modules`, `__pycache__`, `.next`, `.nuxt`, `.output`, `.turbo`, `.parcel-cache`, `.pytest_cache`, `.mypy_cache`, `.ruff_cache`, `.tox`, `.venv`, `venv`, `.gradle`, `.yarn`, `bin`, `obj`, `Pods`, `Carthage`, `bower_components`, and `storybook-static`. The secrets scan is different: in a git work tree it uses `git ls-files` (tracked and non-ignored untracked files), so committed secrets under those directories are still checked. Outside git it uses the same walker as other engines.

Scorecard also skips source files whose first 16 KiB contain `@generated`, or both `Code generated` and `DO NOT EDIT`. Add an explicit path pattern to `scope.include_generated` to scan one of these files or reopen one of the listed directories. If `.gitignore` also excludes the path, remove that ignore rule or add a negation there. `scope.exclude` always takes precedence. When a generated or vendored path is scanned, every report lists it and suggests adding it to `scope.exclude`.

### `[engines]`

| Key | Default | Meaning |
|---|---|---|
| `engines.coverage` | `true` | Run tests with coverage. `false` skips coverage; no CRAP scores are reported and nothing is treated as 0% coverage. |
| `engines.sca` | `true` | Check that imports are declared dependencies. |
| `engines.perf` | `false` | When true, Rust nested loops and `.clone()` inside a loop become `perf.*` findings (disposition `ignore`). Paths under `tests/` or `benches/`, `src/test.rs` / `src/tests.rs`, inline `#[cfg(test)]` / `#[…::test]` items, files from an out-of-line `#[cfg(test)] mod name;` (whole-tree lookup, including `--diff`), and children under that module's directory, are not scanned. Off by default so ordinary Rust does not change the report. |

### `[commands]`

| Key | Default | Meaning |
|---|---|---|
| `commands.lint` | `cargo clippy --workspace` | Lint command for the Rust and `command` packs. At a workspace root, Cargo Clippy checks every member; analyzing a member keeps lint scoped to that package. Empty skips lint. Other packs use their own linter; see [packs](../packs.md). Add `-- -D warnings` to fail on warnings at the workspace root: `cargo clippy --workspace -- -D warnings`. |

### `[mutation]`

| Key | Default | Meaning |
|---|---|---|
| `mutation.mode` | `"off"` | `off`, `diff`, or `full`. `--mutation` overrides. |
| `mutation.max_mutants` | `50` | With more mutants than this in a diff, mutation testing is skipped. |
| `mutation.budget_seconds` | `180` | Time limit for the mutation run. |

### `[llm]`

See [LLM providers](../how-to/llm.md) for setups.

| Key | Default | Meaning |
|---|---|---|
| `llm.enabled` | `false` | Run the LLM review without `--llm on`. With no `--spec`, `--intent` is enough. |
| `llm.backend` | `"ollama"` | `ollama`, `openai-compatible`, or `cursor`. |
| `llm.endpoint` | `"http://127.0.0.1:11434/v1"` | Endpoint for the `ollama` backend. |
| `llm.model` | `"qwen2.5-coder"` | Model name for the chosen backend. |
| `llm.base_url` | `"https://openrouter.ai/api/v1"` | Endpoint for the `openai-compatible` backend. |
| `llm.api_key_env` | `"OPENROUTER_API_KEY"` | Environment variable that holds the key for `openai-compatible`. The key itself never goes in the file. |
| `llm.max_tool_rounds` | `36` | Tool calls the model may make before it must answer. `0` means the default. Not used by `cursor`. |

### `[html]`, `[links]`, `[a11y]`

For the web pack and, for `a11y`, JSX and TSX files.

| Key | Default | Meaning |
|---|---|---|
| `html.enforce` | `"auto"` | `auto` enforces the `html` gate when `fail_on` is the default list or names `html`. `on` or `off` force it. |
| `links.enforce` | `false` | `true` makes a missing internal link fail the run. |
| `a11y.enforce` | `false` | `true` makes accessibility findings fail the run. |
| `a11y.disable` | `[]` | Accessibility rule ids to skip, such as `"contrast"`. See [accessibility](../a11y.md). |
