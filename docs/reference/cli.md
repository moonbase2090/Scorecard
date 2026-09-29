# CLI reference

```text
sc analyze [PATH] [OPTIONS]
sc config path
sc config init [--force]
sc setup
```

## sc analyze

`PATH` is the project directory. Default `.`.

| Flag | Default | Meaning |
|---|---|---|
| `--format FORMAT` | `pretty` on a terminal, `json` otherwise | `json`, `pretty`, `md`, `sarif`, `html`, or `all`. See [Output](#output). |
| `--out PATH` | none | Also write the report to `PATH`. |
| `--fail-on LIST` | `types,tests,crap,secrets,lint` | Comma-separated [gates](gates.md) that fail the run. Overrides `gates.fail_on`. |
| `--pack PACK` | detected | `rust`, `node`, `python`, `bash`, `go`, `java`, `csharp`, `php`, `cpp`, `web`, or `command`. Needed when the tree has two markers. See [packs](../packs.md). |
| `--diff [BASE]` | off | Score only what changed against git `BASE`. Without `BASE`: `HEAD~1`, else `main`. Uncommitted changes count. |
| `--diff-head REV` | worktree | With `--diff`, compare `BASE` to commit `REV` instead of the worktree. |
| `--paths FILE` | off | Score only the source files listed in `FILE`, one per line. Cannot be combined with `--diff`. |
| `--spec FILE` | off | Check that files and public items named in `FILE` exist (`spec` gate). |
| `--mutation MODE` | `off` | `off`, `diff`, or `full`. Runs `cargo mutants` (`mutation` gate). |
| `--llm MODE` | `off` | `off` or `on`. Runs an LLM review of `--spec`, or of `--intent` when there is no spec. See [LLM providers](../how-to/llm.md). |
| `--intent TEXT` | none | What the change is meant to do. Stored on the scorecard. With `--llm on` and no `--spec`, this is enough to run the review. |
| `--budget-seconds N` | `120` | Wall-clock budget for pack commands, in seconds. One clock for the whole run. A command still running at the deadline is killed, including its child processes, and the report records the timeout. |
| `--config PATH` | see [config](config.md#where-sc-looks) | Config file to use. |

Command-line flags override the matching config keys.

## sc config

Print or create `~/.config/sc/analyzer.toml`. See [Configure](../how-to/config.md).

| Command | Meaning |
|---|---|
| `sc config path` | Print the user config path. |
| `sc config init` | Create `~/.config/sc` if needed and write a starter from `analyzer.toml.example`. Does not replace an existing file. |
| `sc config init --force` | Replace an existing file. |

## sc setup

Installs the Scorecard agent skill and registers the `sc-mcp` server for the current user. No flags. See [Agents and MCP](../how-to/agents.md).

## Output

| `--format` | stdout | With `--out PATH` |
|---|---|---|
| `pretty` | Terminal layout | Same text to `PATH` |
| `json` | JSON scorecard | Same JSON to `PATH` |
| `md` | Markdown | Same Markdown to `PATH` |
| `sarif` | SARIF 2.1.0 | Same SARIF to `PATH` |
| `html` | Self-contained HTML page | Same HTML to `PATH` |
| `all` | JSON, then Markdown | `.json`, `.md`, `.sarif`, and `.html` files next to `PATH` |

Every run also writes `.sc/last-scorecard.json`.

## Exit status

| Status | Meaning |
|---|---|
| 0 | Every enforced gate passed |
| 1 | An enforced gate failed |
| 2 | `sc` could not run: missing path, invalid config, a required toolchain missing, or compile or tests timed out |

A skipped coverage, mutation, or LLM engine never causes exit 2.

## Environment

| Variable | Effect |
|---|---|
| `NO_COLOR` | Turns off color |
| `CLICOLOR_FORCE=1` | Forces color |
| `OPENROUTER_API_KEY` | API key for the `openai-compatible` LLM backend (name set by `llm.api_key_env`) |
| `XAI_API_KEY` | When set and `llm.endpoint` is unchanged, `--llm on` uses xAI instead of local Ollama |
