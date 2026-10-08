---
name: scorecard
description: >
  Run the local Scorecard gate (command `sc`) on a project.
  Use when the user asks to check, score, lint, or gate code,
  or when code quality work needs a scorecard before it is called done.
---

# Scorecard

`sc` is a local code quality gate. It reports build, test, coverage and complexity, dependency, secret, lint, and optional policy findings. Run it from the project root unless the user names a different target.

## Install and verify

Install a release binary for the current OS and architecture from the [Scorecard releases](https://github.com/moonbase2090/Scorecard/releases), put `sc` on `PATH`, then verify it:

```sh
sc --version
sc --help
```

For a Rust source install, use `cargo install --locked --path crates/sc-cli`. Install `sc-mcp` separately with `cargo install --locked --path crates/sc-mcp` if MCP tools are needed.

Install the skill for an agent with `sc skills install --agent codex`; see [Skill installation](#skill-installation). `sc setup` installs skill files in legacy locations and registers the optional MCP server.

## Commands

The top-level commands are `sc analyze`, `sc config`, `sc setup`, `sc skills`, and `sc help [COMMAND]`.

### `sc analyze [PATH] [OPTIONS]`

Analyze `PATH` (default `.`). The default output is pretty text on a terminal and JSON when stdout is redirected.

| Flag | Values and default | Purpose |
|---|---|---|
| `--format FORMAT` | `json`, `pretty`, `md`, `sarif`, `html`, `all`; auto-selects pretty/JSON when omitted | Choose stdout and report format. |
| `--out PATH` | none | Also write the report to a file. `all` writes `.json`, `.md`, `.sarif`, and `.html` siblings. |
| `--fail-on LIST` | `types,tests,crap,secrets,lint` | Comma-separated gates whose failure sets exit 1: `types`, `tests`, `crap`, `secrets`, `sca`, `spec`, `mutation`, `lint`, `html`, `links`, or `a11y`. Use `none` by itself to suppress gate failures from the exit code. |
| `--pack PACK` | detected | Select `rust`, `node`, `python`, `bash`, `go`, `java`, `csharp`, `php`, `cpp`, `web`, or `command`. Useful when multiple project markers match. |
| `--diff [BASE]` | off; omitted base tries `HEAD~1`, then `main`, then `master`, skipping a candidate that is `HEAD`, and exits 2 when none remain | Analyze changes against a Git base. Uncommitted changes count. A depth-1 `main` or `master` checkout, a missing base, or any other unresolvable base exits 2 with verdict `fail` and one stderr line naming `fetch-depth: 0`. An explicit `--diff HEAD` still works. |
| `--diff-head REV` | worktree | Compare a diff base to a commit instead of the worktree; use with `--diff`. |
| `--paths FILE` | off | Analyze newline-separated paths. Cannot be combined with `--diff`. |
| `--spec FILE` | off | Check that paths and public items named in a spec exist. |
| `--mutation MODE` | `off`; `diff` or `full` | Run mutation testing. |
| `--llm MODE` | `off`; `on` | Run a read-only spec or intent review using configured provider. |
| `--intent TEXT` | none | Describe the change; used by the optional LLM review and saved in the scorecard. |
| `--budget-seconds N` | `120` | Set the wall-clock budget for pack commands. |
| `--config PATH` | project `analyzer.toml`, then `~/.config/sc/analyzer.toml` | Read a specific config file. |

Command flags override matching config settings. Every run writes `.sc/last-scorecard.json`.

### `sc config`

- `sc config path` prints `~/.config/sc/analyzer.toml`.
- `sc config init` creates the user config directory and writes the starter config only when no file exists.
- `sc config init --force` replaces the user config.

### `sc setup`

`sc setup` installs the skill to `~/.grok/skills/scorecard`, `~/.claude/skills/scorecard`, `~/.cursor/skills/scorecard`, and `~/.agents/skills/scorecard`. When `sc-mcp` is available, it registers the server in `~/.grok/config.toml`, `~/.cursor/mcp.json`, and `~/.claude.json`. It takes no flags and leaves existing skill files unchanged.

`sc help [COMMAND]` prints general help or help for one command, for example `sc help analyze`.

### `sc skills install`

- `--agent AGENT`: `all` (default), `codex`, `shared`, `claude`, `cursor`, `kiro`, `muse`, or `detected`.
- `--check`: inspect selected destinations without writing; exits 1 if a skill is missing or differs.
- `--force`: replace existing files, including edits made by a user. It cannot be combined with `--check`.

`codex` installs in both `~/.codex/skills/scorecard` and shared `~/.agents/skills/scorecard`; `all` includes both. `detected` selects agents whose config directory exists or whose CLI is on `PATH`; it does not create config directories for undetected agents. For Codex it uses existing `.codex` and `.agents` directories, and creates `.codex` only when the Codex CLI is detected and neither directory exists. Existing copies that differ from the embedded skill are preserved unless `--force` is explicit. Automatic install is enabled by default and can be disabled with `install_agent_skills = false` in `~/.config/sc/analyzer.toml` or `SCORECARD_NO_AGENT_SKILLS=1`. Explicit agent targets are manual requests and ignore these automatic-install opt-outs.

## Configuration

Config resolution is `--config PATH`, then `analyzer.toml` in the analyzed directory, then `~/.config/sc/analyzer.toml`. Only the first existing file is used. The complete starter is `analyzer.toml.example` in the repository.

| Section or key | Purpose |
|---|---|
| `install_agent_skills` | Allow automatic agent skill installation; default `true`. Installer hooks read it only from `~/.config/sc/analyzer.toml`. |
| `pack`, `toolchain` | Select the language pack (`detected` by default) and Rustup toolchain (empty uses project toolchain files). |
| `[gates] fail_on`, `crap_threshold`, `new_fn_untested_cc` | Choose failing gates (default `types,tests,crap,secrets,lint`), maximum CRAP score (`30`), and the complexity floor for an untested function (`15`). |
| `[scope] exclude`, `include_generated` | Exclude paths (default empty) or opt selected generated/vendor paths back into scanning (default empty). |
| `[mutation] mode`, `max_mutants`, `budget_seconds` | Configure mutation checks (`off`, `50`, and `180` by default). |
| `[llm] enabled`, `backend`, `endpoint`, `model`, `base_url`, `api_key_env`, `max_tool_rounds` | Configure optional spec/intent review. `backend` is `ollama`, `openai-compatible`, or `cursor`; review is disabled by default. Keep API keys in their named environment variable, not in the config file. |
| `[engines] coverage`, `sca`, `perf` | Enable coverage (`true`), dependency analysis (`true`), and Rust performance checks (`false`). |
| `[commands] lint` | Set the lint command (default `cargo clippy --workspace`); an empty value skips the command lint engine. |
| `[html] enforce`, `[links] enforce`, `[a11y] enforce`, `[a11y] disable` | Configure web and accessibility enforcement (`auto`, `false`, `false`, and no disabled rules by default). |

Unknown config keys are ignored. Check spelling against the config reference before relying on a setting.

## Environment variables

- `HOME` locates user config and agent skill destinations.
- `XDG_CONFIG_HOME` locates Muse configuration for Muse detection; defaults to `~/.config`.
- `PATH` is used to detect agent CLIs and project tools.
- `SCORECARD_NO_AGENT_SKILLS=1` disables automatic detected-agent installation.
- `NO_COLOR`, `CLICOLOR_FORCE`, and `CLICOLOR` control color. `COLUMNS` sets pretty output width.
- `OPENROUTER_API_KEY` or `XAI_API_KEY` can provide credentials for supported optional LLM review setups; `llm.api_key_env` selects the configured key variable.
- `RUSTUP_TOOLCHAIN` can select the inherited Rust toolchain; project config and toolchain files affect precedence.
- `CARGO_LLVM_COV` and `RUSTC_WRAPPER` are inherited by Rust coverage/build tools.
- `VIRTUAL_ENV` and `DOCKER_HOST` are passed through to relevant project tools.

## Read the result

Exit `0` means no selected gate failed. `--fail-on none` deliberately selects no gates. Exit `1` means a selected gate failed. Exit `2` means Scorecard could not run, commonly because a path, config, required toolchain, or gate selection is invalid. Empty or blank `--fail-on` and `gates.fail_on` values are invalid; omit the setting to use the default gates. If an enforced gate fails but is not selected, Scorecard writes one stderr warning with the failed gate names. The report verdict remains `fail`; inspect every gate's `enforced` and `pass` values.

Use `--format json` for automation, `--format pretty` for a terminal, `--format md` for a concise shareable report, `--format sarif` for GitHub code scanning, and `--format html --out report.html` for a self-contained browser report. Findings marked with disposition `fix` need attention; advisory findings may not affect the process exit.

For MCP, `sc-mcp` exposes `analyze_paths`, `analyze_diff`, `list_findings`, and `explain`. After analysis, read `.sc/last-scorecard.json` or use the MCP report tools to inspect findings.

## Workflows

Check a project before calling the work done:

```sh
sc analyze . --format json
```

Check only the current diff and save machine-readable output:

```sh
sc analyze . --diff --format json --out /tmp/scorecard.json
```

For CI, install `sc`, add `.sc/` to `.gitignore`, then run:

```sh
sc analyze . --format sarif --out scorecard.sarif
```

Publish `scorecard.sarif` to the CI code-scanning service when configured. Use a longer `--budget-seconds` if the project test suite needs more than the default budget.

## Troubleshooting

- Exit 2: read stderr and the `engine.unavailable` or `config.invalid` finding. Install the missing project tool, correct the config, or pass the right project path.
- Wrong language pack: pass `--pack` with one of the supported pack names.
- A gate appears but does not change exit status: inspect `--fail-on` and each gate's `enforced` field.
- Coverage is unavailable: install the required coverage tools for the project language; the report identifies a skipped coverage engine.
- Agent skill is not installed: run `sc skills install --agent detected` or select the agent explicitly. Check `install_agent_skills` and `SCORECARD_NO_AGENT_SKILLS` if automatic install was skipped.
- An edited copy remains: this is intentional preservation. Review it, then use `sc skills install --agent AGENT --force` only when replacing it is wanted.

## Do not

- Do not claim a project passed from the exit code alone when the report has other enforced failing gates; inspect the full scorecard.
- Do not edit `.sc/last-scorecard.json` to change the result; rerun analysis after fixing source or configuration.
- Do not put API keys in `analyzer.toml`, command output, or committed files.
- Do not use `--force` on a skill install unless the user wants the existing copy replaced.
- Do not claim checks ran when the report marks an engine skipped or unavailable.
