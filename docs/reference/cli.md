# CLI reference

```text
sc analyze [PATH] [OPTIONS]
sc doctor [PATH] [--pack PACK]
sc tui [PATH] [--inline]    # requires the tui Cargo feature
sc config path
sc config init [--force]
sc setup
sc skills install [--agent AGENT] [--check] [--force]
sc help [COMMAND]
```

## sc analyze

`PATH` is the project directory. Default `.`.

| Flag | Default | Meaning |
|---|---|---|
| `--format FORMAT` | `pretty` on a terminal, `json` otherwise | `json`, `pretty`, `md`, `sarif`, `html`, or `all`. See [Output](#output). |
| `--out PATH` | none | Also write the report to `PATH`. Stdout prints the short summary, including when it is not a terminal. Without this flag, stdout is the chosen `--format`. |
| `--fail-on LIST` | `types,tests,crap,secrets,lint` | Comma-separated [gates](gates.md) whose failures set exit 1. Use `none` by itself to suppress gate failures from the exit code. Empty or blank lists are invalid. Overrides `gates.fail_on`; already enforced failures remain in the report verdict. |
| `--write-baseline` | off | Write each finding's fingerprint to `.sc/baseline.json` under the project. Run it once on the state you accept, then gate with `--baseline`. Cannot be combined with `--baseline`. |
| `--baseline` | off | Exit 1 only when a failing gate fails on a finding not in `.sc/baseline.json`. Known findings stay in the report. Exits 2 with a fix hint when there is no baseline file. Does not gate on metric regressions yet. |
| `--pack PACK` | detected | `rust`, `node`, `python`, `bash`, `go`, `java`, `csharp`, `php`, `cpp`, `web`, or `command`. Needed when the tree has two markers. See [packs](../packs.md). |
| `--diff [BASE]` | off | Score only what changed against git `BASE`. Paths are relative to the project directory, including when that directory sits inside a larger git repository. Python compiles, lints, and checks imports only in changed `.py` files, runs pytest only on changed `test_*.py` and `*_test.py` files, and drops secret findings on unchanged paths. Without `BASE` (AUTO): `HEAD~1`, else `main`, else `master`, skipping a candidate that is the same commit as `HEAD`. When none remain — a shallow clone, a single commit, or a depth-1 clone of `main` or `master` — the run exits 2 with verdict `fail` and one stderr line; fetch full history (`fetch-depth: 0`) or pass `--diff BASE`. A missing explicit base exits 2 the same way and names the ref. An explicit `--diff HEAD` still works. Uncommitted changes count. |
| `--diff-head REV` | worktree | With `--diff`, compare `BASE` to commit `REV` instead of the worktree. |
| `--paths FILE` | off | Score only the source files listed in `FILE`, one per line. Cannot be combined with `--diff`. |
| `--spec FILE` | off | Check that files and public items named in `FILE` exist (`spec` gate). |
| `--mutation MODE` | `off` | `off`, `diff`, or `full`. Runs `cargo mutants` (`mutation` gate). |
| `--llm MODE` | `off` | `off` or `on`. Runs an LLM review of `--spec`, or of `--intent` when there is no spec. See [LLM providers](../how-to/llm.md). |
| `--intent TEXT` | none | What the change is meant to do. Stored on the scorecard. With `--llm on` and no `--spec`, this is enough to run the review. |
| `--budget-seconds N` | `120` | Wall-clock budget for pack commands, in seconds. One clock for the whole run. A command still running at the deadline is killed, including its child processes, and the report records the timeout. |
| `--config PATH` | see [config](config.md#where-sc-looks) | Config file to use. |
| `--quiet` | off | Suppress the progress spinner on stderr. |

While analysis runs, a spinner with the current step and elapsed time animates on stderr when it is a terminal (silent when piped, in CI, with `TERM=dumb`, or from `sc-mcp`); stdout is unchanged, so JSON/SARIF pipes are unaffected. `NO_COLOR` preserves this uncolored progress display, including step and elapsed-time updates during tests and coverage. Use `--quiet` to hide progress.

`sc analyze` skips macOS AppleDouble (`._*`) and `.DS_Store` files in every engine's file walk and writes one stderr line with the count, the first file, and the `find` command that deletes them ([troubleshooting](../troubleshooting.md#ignoring-n-macos-metadata-files)). Test runners and build tools can still read these files and fail.

When a report has more than eight findings, pretty output groups findings that need a fix by rule and suggested action. It shows the five most frequent fix types first, then the findings list. The complete report remains in `.sc/last-scorecard.json` or the path passed to `--out`.

Command-line flags override the matching config keys.

## sc tui

Available only in builds with the `tui` Cargo feature. `PATH` is a project directory,
defaulting to `.`. Reads `PATH/.sc/last-scorecard.json` without running analysis or changing files.
Full-screen mode requires interactive stdin and stdout. `--inline` prints a summary without
entering the alternate screen or reading input. It shows up to eight gates and five findings,
then counts the remaining items. Redirected output is plain text. Exits 0 after displaying
the saved report, including a failing report. Exits 2 for a missing or invalid report,
noninteractive full-screen input/output, terminal setup failure, or output failure.

The gate rail shows PASS, FAIL, or REPORT for advisory failures. Tabs show the total
findings count and the number of engines that ran. The status bar shows the saved verdict.
The Findings table renders only visible rows and shows the selected finding message.
Existing files inside the project have OSC 8 links with escaped `file:` URIs and `#L<line>`
fragments when a positive line number is saved. Terminals decide how to open these links.
Missing files, directories, control-character paths, and paths or symlinks outside the
project stay plain text. Inline mode emits links only on a terminal with a non-dumb `TERM`.

| Key | Action |
|---|---|
| Up/Down, `k`/`j` | Select a finding on Findings, otherwise a gate |
| Home/End | Select the first/last finding or gate |
| PageUp/PageDown | Move one visible page on Findings |
| Tab/Shift+Tab, Right/Left, `l`/`h` | Cycle tabs |
| `q`, Ctrl+C | Quit and restore the terminal |

The initial terminal size sets the layout. Reopen after resizing. Below 60 columns or
12 rows, the browser shows a size hint. See [Browse a saved analysis](../how-to/tui.md).

## sc config

Print or create `~/.config/sc/analyzer.toml`. See [Configure](../how-to/config.md).

| Command | Meaning |
|---|---|
| `sc config path` | Print the user config path. |
| `sc config init` | Create `~/.config/sc` if needed and write a starter from `analyzer.toml.example`. Does not replace an existing file. |
| `sc config init --force` | Replace an existing file. |

## sc doctor

Lists the tools the project's language pack needs, marks each `ok` or missing, and prints the install command for every missing one. A missing required tool is one an enforced gate cannot run without; a missing optional tool only disables an extra check. Requirements follow the tree: `cc` is required only with `.c` files and `g++` only with C++ files, `pytest` only with a pytest suite, and `uv` is required to run a pytest suite in a project with `uv.lock` or `.venv`, even when system pytest is installed. With a suite and coverage enabled, doctor also checks for `pytest-cov` or `coverage.py`; uv supplies these automatically for uv projects. Elsewhere, either installed coverage tool is enough. `uv` can also supply `ruff`. The pack comes from the same detection as `sc analyze`, including `pack` in `analyzer.toml`. When several manifests match, `sc doctor` stops like `sc analyze` does; pass `--pack`. Install commands use `brew` on macOS and `apt-get` where it exists; otherwise they give the Debian package name.

| Exit | Meaning |
|---|---|
| `0` | Every required tool is present. |
| `1` | A required tool is missing. |
| `2` | No pack matches the path, several packs match, or `--pack` is unknown. |

```text
$ sc doctor
rust pack
  ok        cargo
  MISSING   clippy (required, used for lint)
            install: rustup component add clippy
```

## sc setup

Installs the Scorecard agent skill for the legacy agent locations and registers the `sc-mcp` server for the current user. It leaves existing skill files unchanged. No flags. See [Agents and MCP](../how-to/agents.md).

## sc skills install

Installs the embedded Scorecard skill. Default `--agent all` targets all supported integrations. Run `sc skills install --help` for the supported agent identifiers. `--agent codex` writes both Codex and shared locations. `--agent detected` targets only existing config directories or agents whose CLI is on `PATH`; when Codex is detected, it uses existing Codex/shared directories and creates `~/.codex` only when Codex is detected by its CLI and neither directory exists. This mode is used by the macOS package hook and honors automatic-install opt-outs.

| Flag | Default | Meaning |
|---|---|---|
| `--agent AGENT` | `all` | Agent identifier, `all`, or `detected`. Run `sc skills install --help` for supported values. |
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

With `--out`, stdout is the short summary and the third column is what is written to the file. Without `--out`, stdout is the second column.

Every run also writes `.sc/last-scorecard.json`.

SARIF stores the report verdict in `runs[0].properties.scorecardVerdict` and each gate's `pass` and `enforced` flags in `runs[0].properties.scorecardGates`.

JSON includes the selected gates in `fail_on`. SARIF stores the same list in `runs[0].properties.fail_on`. If an enforced gate fails but is not selected, `sc` writes one stderr warning with the failed gate names.

Scorecard excludes its saved report, cache and coverage files under `.sc/`, and report files selected by `--out` from the dirty-tree check. Other changes set `git.dirty` and appear in `git.dirty_paths`.

## Exit status

| Status | Meaning |
|---|---|
| 0 | No selected gate failed. `--fail-on none` deliberately selects no gates. If an enforced gate failed, `sc` names it in one stderr warning and the report verdict remains `fail`. With `--baseline`, a failing gate that fails only on known findings exits 0 and the report verdict remains `fail`. |
| 1 | A selected gate failed |
| 2 | `sc` could not run: missing path, invalid config (including an empty `--fail-on` or `gates.fail_on`), a required toolchain missing, or compile or tests timed out |

A skipped coverage, mutation, or LLM engine never causes exit 2.

## Environment

| Variable | Effect |
|---|---|
| `NO_COLOR` | Turns off color |
| `CLICOLOR_FORCE=1` | Forces color |
| `OPENROUTER_API_KEY` | API key for the `openai-compatible` LLM backend (name set by `llm.api_key_env`) |
| `XAI_API_KEY` | When set and `llm.endpoint` is unchanged, `--llm on` uses xAI instead of local Ollama |
