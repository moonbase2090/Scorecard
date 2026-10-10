# Scorecard on Windows

Status: plan. Windows is not a supported platform. This page does not say that `sc` runs on Windows, and it does not change the README, the release assets, or CI required checks.

Audited on `develop` at `7d8d4274d6250b2ab3578ed70fcb6e9f10c056c3` (2026-10-10). The repository has no roadmap file. The README quickstart names only Darwin and Linux targets. `.github/workflows/ci.yml` and `release.yml` have no Windows job.

Real-Windows proof is a later step, on the PC, by windows-1. Grok signs off on this plan before those leaves start.

## Decisions

1. The first target is `x86_64-pc-windows-msvc` only.
2. Unix behavior stays as it is. Windows work is new branches, not `cfg` edits that change Unix results.
3. GitHub-hosted `windows-latest` is report-only. Hosted Windows minutes are billing-blocked, so a green PC run is the proof. The job is not a required check and cannot block Apple or Linux releases.
4. The unsigned zip ships before an MSI. Signing is later, through Azure Artifact Signing (`moonbase2090-signing`).
5. The README and the quickstart gain a Windows download only after windows-1 posts `sc analyze` output and screenshots from the PC.

## Already portable

Report paths are stored with `/` separators. `scope`, `pack`, `coverage`, the file walk, compile diagnostics, and the Python pack replace `\` before they record a path. The walk uses the `ignore` crate with `git_ignore(true)`, `git_global(false)`, and `require_git(false)`, so a project `.gitignore` applies without a machine-wide ignore file.

`sc-mcp` frames stdio as a JSON line or a `Content-Length` body. That loop has no Unix-only code.

`command.rs` puts the child in its own process group and kills the group only on Unix. On other targets `stop_tree` calls `child.kill()` on the direct child. `pretty.rs` reads the column count with `TIOCGWINSZ` only on Unix and otherwise uses `COLUMNS` or 80. `skills.rs` treats any regular file as executable when the target is not Unix.

Rust `cargo check` and `cargo test` already go through `Command::new("cargo")` once `host_has("cargo")` is true (`pipeline.rs` `run_check`, `run_tests`, `command.rs` `run_cargo`).

## Gap audit

### Process spawning and kill/timeouts

`run_cmd` polls `try_wait` and calls `stop_tree` at the deadline (`crates/sc-engines/src/command.rs`). On Unix the child is started with `process_group(0)` and the timeout sends `SIGKILL` to `-pid`, so grandchildren die with the group. On Windows only the direct child is killed. `cargo` then leaves `rustc` running, and a shell would leave whatever it started.

Windows fix: create a job object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, assign the spawned process to it, and close the job on timeout. Unix keeps the process group. A timeout must still surface `CommandError::Timeout` and the existing `--budget-seconds` message.

Many engines never reach `Command::new("cargo")`. They run `sh -c` (`toolchain.rs` `shell`, `pipeline.rs` `run_shell`, `python.rs` `shell`, `poly_cc.rs`). There is no `sh` on a stock Windows install. Those calls fail at spawn today.

### Paths and `\` separators

Stored report paths already use `/`. The gaps are user-level paths and arguments, not finding locations.

`user_config_path` reads only `HOME` (`crates/sc-core/src/config.rs`). `sc setup` returns 2 with `HOME is not set` when that variable is missing (`crates/sc-cli/src/setup.rs`). A normal Windows PowerShell or `cmd` session does not set `HOME`.

POSIX quoting is used for shell arguments (`shell_quote` in `toolchain.rs`, `shell_quote_arg` in `command.rs`): wrap in single quotes and escape `'`. That quoting is wrong for `cmd.exe`. Do not send those strings through `cmd /c`.

Docker lookup forces `DOCKER_HOST=unix://...` for `/var/run/docker.sock` and `~/.docker/desktop/docker.sock` (`toolchain.rs` `docker_host`). Docker Desktop on Windows listens on a named pipe (`npipe:////./pipe/docker_engine`). The first leaves do not wrap engines in Docker on Windows. Leave `DOCKER_HOST` alone unless it is already set.

### `.gitignore`

The walker already honors nested `.gitignore` files and skips the usual build directories. Secrets listing uses `git ls-files` when the directory is a work tree (`sc-graph` `walk.rs`). Nothing in that path is `cfg(unix)`.

Verify on the PC before changing it: CRLF-ended ignore files, patterns written with `/`, `core.ignorecase`, and paths that `git ls-files` quotes. Fix only a miss the PC run shows. Do not reimplement the `ignore` crate.

### ANSI and the VT console

Pretty output uses `anstyle` ANSI sequences (`crates/sc-cli/src/pretty.rs`). Color follows `NO_COLOR`, `CLICOLOR_FORCE`, `CLICOLOR`, and whether stdout is a terminal. A Windows console prints those sequences as text until `ENABLE_VIRTUAL_TERMINAL_PROCESSING` is set.

Use the same fix Prismattyc uses in `enable_console_vt` (`prismattyc-mux` `platform.rs`): `GetConsoleMode` / `SetConsoleMode` on stdout and stderr with output processing and `DISABLE_NEWLINE_AUTO_RETURN`. Color stays off when the console rejects the mode, unless `CLICOLOR_FORCE` is set to something other than `0`. A pipe or a file stays plain, as it does now.

Width: when `COLUMNS` is unset, read the console window with `GetConsoleScreenBufferInfo` instead of falling through to 80. Keep the 40-column minimum.

### Skill and MCP setup paths

`sc setup` and `sc skills install` write under `$HOME`:

| What | Path today |
|---|---|
| User config | `$HOME/.config/sc/analyzer.toml` |
| Skill version stamp | `$HOME/.config/sc/agent-skills-version` |
| Claude Code skill | `$HOME/.claude/skills/scorecard` |
| Cursor skill | `$HOME/.cursor/skills/scorecard` |
| Codex skill | `$HOME/.codex/skills/scorecard` |
| Shared skill | `$HOME/.agents/skills/scorecard` |
| Kiro skill | `$HOME/.kiro/skills/scorecard` |
| Grok skill | `$HOME/.grok/skills/scorecard` |
| Grok MCP | `$HOME/.grok/config.toml` |
| Cursor MCP | `$HOME/.cursor/mcp.json` |
| Claude Code MCP | `$HOME/.claude.json` |

Muse config also checks `XDG_CONFIG_HOME` and `$HOME/.muse`. The version stamp ignores `XDG_CONFIG_HOME` and always uses `$HOME/.config/sc`.

Windows resolution for the later leaf:

- Profile is `HOME` when it is set, otherwise `USERPROFILE`.
- App data is `APPDATA`, otherwise `<profile>\AppData\Roaming`.
- User config and the skill version stamp go in `%APPDATA%\sc\`.
- Agent skill directories and the three MCP files stay under the profile. Claude Code, Cursor, Codex, Kiro, and Grok store those beside the profile, not under `%APPDATA%`. This leaf does not add Claude Desktop's `%APPDATA%\Claude\claude_desktop_config.json`.

`sc-mcp` lookup (`setup.rs` `mcp_bin`) runs `sh -c 'command -v sc-mcp'` and then checks a sibling named `sc-mcp` and `%HOME%\.cargo\bin\sc-mcp`. On Windows the sibling of `sc.exe` is `sc-mcp.exe`. The lookup has to try the `.exe` name, and `PATH` search has to honor `PATHEXT`.

### Engine tool discovery

`which` is `sh -c 'command -v NAME'` in `toolchain.rs`, `python.rs`, and `setup.rs`. Pack plans in `toolchain_plans.rs` call that before they build a shell script. The scripts use `mkdir -p`, `$PWD`, `status=$?`, `command -v`, `&&`, and POSIX quotes.

| Tool | How it is found today | Windows note |
|---|---|---|
| cargo | `command -v cargo`, then `Command::new("cargo")` | `cargo.exe` is on `PATH` after rustup. Discovery is the blocker, not the spawn. |
| rustfmt, clippy, llvm-cov | shell script whose first token is `cargo` (`run_shell`) | Needs a direct `cargo` spawn or a real `sh`. |
| python | hardcoded `python3` | Stock installs expose `py` and `python.exe`, not `python3`. |
| node | `command -v node`, then `node --check` inside `sh` | `node.exe` is fine; the shell wrapper is not. |
| npm, c8, nyc | `command -v`, then `npm test` inside `sh` | `npm` is `npm.cmd`. `Command` finds it via `PATHEXT`; `command -v` does not. |
| java, javac, mvn, gradle | shell script (`mvn`, `gradle`, `javac`) | `mvn` and `gradle` are `.cmd` / `.bat`. |
| dotnet | shell script with `DOTNET_CLI_HOME="$PWD/.sc/dotnet"` and `status=$?` | `dotnet.exe` exists. The script is POSIX. |
| go, php, cc, g++ | same `via()` shell path | Same discovery gap. `cc` is not the MSVC driver. |

`tool_missing` already treats `not recognized` as a missing tool (`toolchain.rs`). That string is what `cmd` prints. It never runs if `sh` itself is missing.

First behavior rule: look up a tool by walking `PATH` and `PATHEXT` (`.exe`, `.cmd`, `.bat`, plus the bare name). Do not spawn `sh` for that lookup. Unix keeps `command -v`.

Packs whose command is a POSIX pipeline stay unavailable on Windows until a later leaf ports that pack. The finding stays `engine.unavailable` and names the missing tool or the missing POSIX shell. It must not be a raw spawn error with an empty stderr. The Rust pack is first, because `testdata/good_crate` is Rust and check/test already spawn `cargo` directly.

### `sc-mcp`

The server binary is portable once it is built with the MSVC toolchain. The work around it is not:

- `mcp_bin` misses `sc-mcp.exe`.
- Registration writes the absolute path into JSON and TOML. `setup.rs` already escapes `\`, so a path like `C:\Users\...\sc-mcp.exe` can be stored. Confirm that on the PC.
- The process it launches is `sc`. That child has the same `PATH`, config, and console gaps as a terminal run.

No MCP protocol change.

### Tests that do not compile or do not run

`cargo test --workspace` is not green on Windows today. These tests use Unix-only APIs outside `#[cfg(unix)]`:

- `crates/sc-cli/src/skills.rs`: `fake_muse` calls `std::os::unix::fs::PermissionsExt`.
- `crates/sc-engines/src/pack.rs`: symlink and mode-bit tests call `std::os::unix::fs`.

Other tests compile and then fail because they run `sh`, `python3`, or a shell fixture. The warm coverage test in `pipeline.rs` is already `#[cfg(unix)]`.

The compile leaf gates those tests with `#[cfg(unix)]`. It does not delete them and it does not change what Ubuntu runs. The Windows test command is still `cargo test --workspace --locked`.

### CI

`ci.yml` jobs are `ubuntu-latest` or `macos-14`. `paths-ignore` skips Markdown, so this plan PR does not run the test job. `docs.yml` does run on this PR and executes `scripts/check-docs.py`.

The CI leaf adds `check-windows` on `windows-latest` with `continue-on-error: true`. Steps: fmt, clippy, `cargo test --workspace --locked`. It does not install kcov, php, or apt packages, and it does not run the bash scripts under `scripts/`. A comment in the workflow states that the job is report-only and that the PC run is the gate. Do not add the job to branch protection.

### Packaging

`release.yml` builds a universal Apple disk image and two Linux `.tar.gz` archives (`x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`). `publish` runs only when the Apple job reports `signed=true`.

The release leaf adds a Windows job that builds `--target x86_64-pc-windows-msvc` for `sc-cli` and `sc-mcp` and writes:

`sc-v<version>-x86_64-pc-windows-msvc.zip`

Nested layout, matching the tar archives:

```text
sc-v<version>-x86_64-pc-windows-msvc/
  sc.exe
  sc-mcp.exe
  LICENSE
  README.md
```

`scripts/check-archive-layout.py` understands `.tar.gz` and requires the names `sc` and `sc-mcp`. Extend it for this zip and these two `.exe` names. Do not rename the Linux binaries.

`publish` uploads the zip when the Windows job succeeds. `publish` does not `needs` that job. A billing skip must not stop the Apple and Linux assets.

The MSI is a later PR. It is unsigned. Signing waits on Azure Artifact Signing via `moonbase2090-signing`, after the unsigned zip has been installed on the PC.

## Leaves after this plan

Each leaf is its own PR into `develop`, labeled `leaf`. None of them edit the README to claim Windows support.

1. Compile and test. `#[cfg(unix)]` on the Unix-only tests named above, plus any further compile break the PC build shows. `cargo test --workspace --locked` passes on the PC. Ubuntu CI stays required and unchanged.
2. Tool lookup and the Rust pack. `PATH`/`PATHEXT` lookup so `host_has("cargo")` is true without `sh`. `sc analyze` on `testdata/good_crate` completes check and test through `cargo.exe`. Lint and other `sh -c` engines report `engine.unavailable` with one clear line when `sh` is absent.
3. Process-tree timeout. Job object kill, with a test that a grandchild does not outlive the deadline.
4. Console. VT mode and console width, covered by a unit test of the mode decision (accepted, rejected, pipe).
5. Config and skills. `%APPDATA%\sc` and profile fallback, plus `sc-mcp.exe` discovery.
6. CI. The report-only `check-windows` job.
7. Release zip. The asset above, still not a publish dependency.

windows-1 runs the proof on the PC after leaf 2, and again after the zip exists: `sc analyze` on `testdata/good_crate` and on a real repository, with the command output and screenshots of the pretty report. Name the console host (Windows Terminal or conhost). That proof is the point at which a later PR may mention Windows in the README.

## Out of scope

- Claiming Windows support in the README, the quickstart, or the changelog's user-facing "supported platforms" wording before the PC proof.
- `aarch64-pc-windows-msvc`.
- A required GitHub-hosted Windows check.
- Porting every language pack's POSIX pipeline in the first leaves.
- MSI, code signing, and `moonbase2090-signing` wiring.
- Rewriting `.gitignore` handling without a PC failure.
