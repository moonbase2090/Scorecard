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
| `pack` | detected | Language pack when the tree has more than one marker. Same values as `--pack`. |
| `toolchain` | empty | Rustup channel for check, test, coverage, and lint (`1.85.0`, `stable`, …). When set, Cargo uses that channel. When empty, a `rust-toolchain.toml` / `rust-toolchain` at or above the project clears an inherited `RUSTUP_TOOLCHAIN` so rustup reads the file; with no file, the inherited variable is left alone. |

### `[gates]`

| Key | Default | Meaning |
|---|---|---|
| `gates.fail_on` | `["types", "tests", "crap", "secrets", "lint"]` | [Gates](gates.md) that fail the run. `--fail-on` overrides. An unknown gate name is an error (exit 2). |
| `gates.crap_threshold` | `30` | A function with a [CRAP](../crap.md) score above this fails the `crap` gate. |
| `gates.new_fn_untested_cc` | `15` | A function at or above this complexity with 0% measured coverage fails the `crap` gate (`complexity.untested`). With `--diff`, only new functions count. |

### `[scope]`

| Key | Default | Meaning |
|---|---|---|
| `scope.exclude` | `["target/**", "generated/**"]` | Globs left out of complexity, CRAP, and the secrets walk. Complexity and CRAP match `target/**` and `generated/**` at any depth, so `src/generated` is not scored. The secrets walk anchors a pattern that does not start with `**/` to the project root, so `src/target/keys.py` is still scanned. `**/target/**` skips that path in the secrets walk too. |

### `[engines]`

| Key | Default | Meaning |
|---|---|---|
| `engines.coverage` | `true` | Run tests with coverage. `false` skips coverage; no CRAP scores are reported and nothing is treated as 0% coverage. |
| `engines.sca` | `true` | Check that imports are declared dependencies. |
| `engines.perf` | `false` | When true, Rust nested loops and `.clone()` inside a loop become `perf.*` findings (disposition `ignore`). Paths under `tests/` or `benches/`, `src/test.rs` / `src/tests.rs`, inline `#[cfg(test)]` / `#[…::test]` items, and files from an out-of-line `#[cfg(test)] mod name;`, are not scanned. Off by default so ordinary Rust does not change the report. |

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
