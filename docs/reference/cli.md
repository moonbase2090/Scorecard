# CLI reference

```text
sc analyze [PATH] [OPTIONS]
sc config path
sc config init [--force]
sc setup
sc skills install [--agent all|codex|shared|claude|cursor|kiro|muse|detected] [--check] [--force]
sc help [COMMAND]
```

## sc analyze

`PATH` is the project directory. Default `.`.

| Flag | Default | Meaning |
|---|---|---|
| `--format FORMAT` | `pretty` on a terminal, `json` otherwise | `json`, `pretty`, `md`, `sarif`, `html`, or `all`. See [Output](#output). |
| `--out PATH` | none | Also write the report to `PATH`. |
| `--fail-on LIST` | `types,tests,crap,secrets,lint` | Comma-separated [gates](gates.md) whose failures set exit 1. Overrides `gates.fail_on`; already enforced failures remain in the report verdict. |
| `--pack PACK` | detected | `rust`, `node`, `python`, `bash`, `go`, `java`, `csharp`, `php`, `cpp`, `web`, or `command`. Needed when the tree has two markers. See [packs](../packs.md). |
| `--diff [BASE]` | off | Score only what changed against git `BASE`. Paths are relative to the project directory, including when that directory sits inside a larger git repository. Without `BASE`: `HEAD~1`, else `main`. Uncommitted changes count. |
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

Installs the Scorecard agent skill for the legacy agent locations and registers the `sc-mcp` server for the current user. It leaves existing skill files unchanged. No flags. See [Agents and MCP](../how-to/agents.md).

## sc skills install

Installs the embedded Scorecard skill. Default `--agent all` targets Codex (`~/.codex` and shared `~/.agents` skills), Claude Code, Cursor, Kiro, and Muse. `--agent codex` writes both Codex and shared locations. `--agent detected` targets only existing config directories or agents whose CLI is on `PATH`; when Codex is detected, it uses existing Codex/shared directories and creates `~/.codex` only when Codex is detected by its CLI and neither directory exists. This mode is used by the macOS package hook and honors automatic-install opt-outs.

| Flag | Default | Meaning |
|---|---|---|
| `--agent AGENT` | `all` | `all`, `codex`, `shared`, `claude`, `cursor`, `kiro`, `muse`, or `detected`. |
| `--check` | off | Check selected targets without writing. Exits 1 if a skill is missing or differs from the embedded copy. Cannot be combined with `--force`. |
| `--force` | off | Replace an existing skill, including a user-edited copy. |

Existing files that differ from the embedded skill are preserved unless `--force` is set. See [Agents and MCP](../how-to/agents.md) for destinations and automatic installation.

## sc help

Print general help with `sc help`, or command help with `sc help analyze`.

## Output

| `--format` | stdout | With `--out PATH` |
|---|---|---|
| `pretty` | Terminal layout | Same text to `PATH` |
| `json` | JSON scorecard | Same JSON to `PATH` |
| `md` | Markdown | Same Markdown to `PATH` |
| `sarif` | SARIF 2.1.0. Only `secrets.*` findings are level `error`; every other rule is `warning` or `note`. | Same SARIF to `PATH` |
| `html` | Self-contained HTML page | Same HTML to `PATH` |
| `all` | JSON, then Markdown | `.json`, `.md`, `.sarif`, and `.html` files next to `PATH` |

Every run also writes `.sc/last-scorecard.json`.

SARIF stores the report verdict in `runs[0].properties.scorecardVerdict` and each gate's `pass` and `enforced` flags in `runs[0].properties.scorecardGates`.

Scorecard excludes its saved report, cache and coverage files under `.sc/`, and report files selected by `--out` from the dirty-tree check. Other changes set `git.dirty` and appear in `git.dirty_paths`.

## Exit status

| Status | Meaning |
|---|---|
| 0 | No selected gate failed. The report verdict can still be `fail` if another enforced gate failed. |
| 1 | A selected gate failed |
| 2 | `sc` could not run: missing path, invalid config, a required toolchain missing, or compile or tests timed out |

A skipped coverage, mutation, or LLM engine never causes exit 2.

## Environment

| Variable | Effect |
|---|---|
| `NO_COLOR` | Turns off color |
| `CLICOLOR_FORCE=1` | Forces color |
| `OPENROUTER_API_KEY` | API key for the `openai-compatible` LLM backend (name set by `llm.api_key_env`) |
| `XAI_API_KEY` | When set and `llm.endpoint` is unchanged, `--llm on` uses xAI instead of local Ollama |
