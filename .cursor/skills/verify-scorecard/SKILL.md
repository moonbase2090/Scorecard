---
name: verify-scorecard
description: Verify Scorecard CLI changes through real analysis, gate exits, report files, and path or Git-diff scope. Use for PR proof and when maintaining the verification feature map.
---

# Verify Scorecard

Read [the feature index](features/README.md), then drive the entries affected by the change. The primary surface is `sc`; MCP, installation, and optional external engines need additional proof when touched.

## Launch

Use an isolated checkout and a dedicated Bash shell from its root. Build the checkout's binaries; an installed `sc` may be an older release.

```bash
set -e
cargo build --locked -p sc-cli -p sc-mcp
sc_repo=$(pwd -P)
sc_binary="$sc_repo/target/debug/sc"
sc_mcp="$sc_repo/target/debug/sc-mcp"
sc_tmp=${TMPDIR:-/tmp}
sc_evidence=$(mktemp -d "${sc_tmp%/}/sc-proof.XXXXXX")
sc_scratch=$(mktemp -d "${sc_tmp%/}/sc-drive.XXXXXX")
export sc_repo sc_binary sc_mcp sc_evidence sc_scratch
sc_cleanup() {
  rm -rf -- "$sc_scratch"
  test ! -e "$sc_scratch"
  test -d "$sc_evidence"
  printf 'scratch removed; evidence preserved\n' > "$sc_evidence/cleanup.txt"
}
trap 'sc_status=$?; sc_cleanup || sc_status=1; exit "$sc_status"' EXIT
cp -R testdata/web_site "$sc_scratch/site"
printf 'pack = "web"\n[html]\nenforce = "on"\n' > "$sc_scratch/analyzer.toml"
sc_config="$sc_scratch/analyzer.toml"
export sc_config
git rev-parse HEAD > "$sc_evidence/revision.txt"
printf '%s\n' "$sc_evidence"
```

Each invocation is a short-lived process. There is no server or shared terminal to keep running. Each drive below uses a fresh process against this run's copied fixture, an explicit config, and automatic agent installation disabled. Keep real user config and agent registrations unchanged.

For the repository's required build/test/self-analysis gates, follow [CONTRIBUTING.md](../../../CONTRIBUTING.md) and [the review policy](../../../REVIEW_POLICY.md). The web walkthrough does not replace language-pack, mutation, coverage, or release tests.

## Doctor

Before the first drive and after any surprise, confirm the binary identity and CLI entry points. This reads version/help information only. The helper calls this check before every analysis invocation.

```bash
sc_doctor() {
  env SCORECARD_NO_AGENT_SKILLS=1 "$sc_binary" --version > "$sc_evidence/sc-version.txt" &&
  "$sc_mcp" --version > "$sc_evidence/mcp-version.txt" &&
  env SCORECARD_NO_AGENT_SKILLS=1 "$sc_binary" analyze --help > "$sc_evidence/analyze-help.txt"
}
sc_doctor
python3 - <<'CHECK'
import os, pathlib, tomllib
root = pathlib.Path(os.environ['sc_repo'])
evidence = pathlib.Path(os.environ['sc_evidence'])
version = tomllib.loads((root / 'Cargo.toml').read_text())['workspace']['package']['version']
assert (evidence / 'sc-version.txt').read_text().strip() == 'sc ' + version
assert (evidence / 'mcp-version.txt').read_text().strip() == version
CHECK
```

If the binary or version is wrong, rebuild and repeat Doctor. After a failed recipe, retain its evidence, run Cleanup, and begin again at Launch; inspect an actual product failure instead of changing expected results to match it.

## Drive

Define this capture helper in the same shell, then execute the Bash blocks in the chosen feature file. `sc_run` records the action, stdout, stderr, and exit status, and rejects an unexpected exit. Each feature also checks the result's meaning.

```bash
sc_run() {
  local label="$1" expected="$2" status
  shift 2
  sc_doctor || return 1
  printf '%q ' "$sc_binary" analyze "$@" --config "$sc_config" --llm off > "$sc_evidence/$label.command"
  printf '\n' >> "$sc_evidence/$label.command"
  if env SCORECARD_NO_AGENT_SKILLS=1 "$sc_binary" analyze "$@" --config "$sc_config" --llm off > "$sc_evidence/$label.stdout" 2> "$sc_evidence/$label.stderr"; then
    status=0
  else
    status=$?
  fi
  printf '%s\n' "$status" > "$sc_evidence/$label.exit"
  test "$status" -eq "$expected"
}
```

Use a fresh Launch for each feature, or run the map in index order. Each recipe restores the fixture changes it makes. Run changed interactive progress behavior in a real PTY as well; redirected capture cannot prove a spinner or terminal-only rendering.

## Evidence

Retain the printed `sc-proof.*` directory. Include the revision, command, exit, relevant stdout/stderr, parsed gate/finding assertions, and report-file checks in PR proof. Record which mapped entries ran and which remain unverified. A failed gate can coexist with exit 0 under `--fail-on none`; inspect the report verdict and enforced gates.

Exercise the real CLI and observe `.sc/last-scorecard.json` and requested output files. These recipes use no mocks or external review provider. Confirm any additional dry-run/check mode's filesystem and network effects separately. Review artifacts for secrets and personal paths before copying excerpts into a public PR; keep raw evidence outside Git.

## Cleanup

Launch installs an EXIT trap for failed attempts. Run the block below after successful attempts before leaving the shell. Only the scratch directory created by Launch is removed; saved evidence remains. All commands above run in the foreground, so there is no process to kill. If an additional drive starts a process, record its PID and stop that specific process during teardown.

```bash
sc_cleanup
trap - EXIT
test -s "$sc_evidence/revision.txt"
test -s "$sc_evidence/sc-version.txt"
find "$sc_evidence" -type f -print
```

## Commit hook

For hook or installer changes, run `python3 scripts/test-pre-commit.py` with Git,
Python 3, Rust with rustfmt, and Node.js with npm. The first web check downloads
Biome 2.6.2. Require all ten real-commit cases to pass: installer reuse, Rust and
web partial staging in both directions, unusual names, unstaged child modules,
staged Biome configuration, web rename/deletion, and unrelated commits. Verify
rejections preserve HEAD, index blobs, and working files; accepted commits contain
the staged blobs and preserve unstaged edits. Rust files outside Cargo targets
must still be checked. Repeat this script from the exact source archive in the
allocated clean macOS guest and retain its output.

## Helpers and maintenance

`sc_cleanup`, `sc_doctor`, and `sc_run` above are shell helpers, not installed commands. They use Bash, Python 3.11 or newer, Git, and the built binaries. The web recipes need no language-pack services or credentials. The scope recipe uses Rust and needs `cargo-llvm-cov` plus `llvm-tools-preview`; see the pack prerequisites before that drive.

Run `/maintain-verification-skill` when a PR changes build commands, runtime behavior, test prerequisites, or a mapped entry point. Its source review and live pass cover every feature file. Add a feature recipe when a change introduces another user-facing path.

### Windows process-tree cleanup

For changes to Windows command containment, run on a native Windows checkout:

```powershell
py scripts/verify-windows-process-tree.py
```

The driver requires each named test to execute and pass. It checks that a running grandchild is stopped on timeout, timeout stderr is retained, and a grandchild cannot keep output pipes open after its parent succeeds. Save the exact revision and output. Unix regression proof is `cargo test --locked -p sc-engines command::tests`. Cross-compilation is supplemental; native Windows runtime proof remains a release gate.
