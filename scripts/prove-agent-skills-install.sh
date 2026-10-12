#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if [ -n "${SCORECARD_BIN:-}" ]; then
  sc=$SCORECARD_BIN
else
  cargo build --locked -p sc-cli --manifest-path "$repo_root/Cargo.toml"
  sc="$repo_root/target/debug/sc"
fi
tmp=$(mktemp -d "${TMPDIR:-/tmp}/sc-agent-skills.XXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

home="$tmp/home"
bin="$tmp/bin"
mkdir -p "$home/.codex" "$home/.claude" "$bin"
cat >"$bin/cursor-agent" <<'EOF'
#!/bin/sh
exit 0
EOF
chmod +x "$bin/cursor-agent"

printf '%s\n' '== detected agents install =='
HOME="$home" XDG_CONFIG_HOME="$home/.config" PATH="$bin" SCORECARD_NO_AGENT_SKILLS=0 "$sc" skills install --agent detected
test -f "$home/.codex/skills/scorecard/SKILL.md"
test -f "$home/.claude/skills/scorecard/SKILL.md"
test -f "$home/.cursor/skills/scorecard/SKILL.md"
test ! -e "$home/.agents"
test ! -e "$home/.kiro"
test ! -e "$home/.muse"
expected_version=$("$sc" --version | awk '{print $2}')
test "$(cat "$home/.config/sc/agent-skills-version")" = "$expected_version"

printf '%s\n' '== explicit Codex target includes shared skills =='
codex_home="$tmp/explicit-codex"
mkdir -p "$codex_home"
HOME="$codex_home" XDG_CONFIG_HOME="$codex_home/.config" PATH="$tmp/empty-bin" "$sc" skills install --agent codex
test -f "$codex_home/.codex/skills/scorecard/SKILL.md"
test -f "$codex_home/.agents/skills/scorecard/SKILL.md"

printf '%s\n' '== same version is a no-op =='
before=$(cksum "$home/.codex/skills/scorecard/SKILL.md" | awk '{print $1 ":" $2}')
out=$(HOME="$home" XDG_CONFIG_HOME="$home/.config" PATH="$bin" SCORECARD_NO_AGENT_SKILLS=0 "$sc" skills install --agent detected)
after=$(cksum "$home/.codex/skills/scorecard/SKILL.md" | awk '{print $1 ":" $2}')
test "$before" = "$after"
case "$out" in *'already installed'*) ;; *) echo "expected version no-op, got: $out" >&2; exit 1 ;; esac

printf '%s\n' '== version change triggers install =='
printf '%s\n' '0.0.0' >"$home/.config/sc/agent-skills-version"
out=$(HOME="$home" XDG_CONFIG_HOME="$home/.config" PATH="$bin" SCORECARD_NO_AGENT_SKILLS=0 "$sc" skills install --agent detected)
case "$out" in *'already installed'*) echo "old version was incorrectly skipped" >&2; exit 1 ;; esac
test "$(cat "$home/.config/sc/agent-skills-version")" = "$expected_version"

printf '%s\n' '== edited copy is preserved =='
printf '%s\n' 'user edit' >"$home/.codex/skills/scorecard/SKILL.md"
HOME="$home" XDG_CONFIG_HOME="$home/.config" PATH="$bin" "$sc" skills install --agent codex
test "$(cat "$home/.codex/skills/scorecard/SKILL.md")" = 'user edit'

printf '%s\n' '== environment opt-out =='
env_home="$tmp/env-opt-out"
mkdir -p "$env_home/.claude"
HOME="$env_home" XDG_CONFIG_HOME="$env_home/.config" PATH="$bin" SCORECARD_NO_AGENT_SKILLS=1 "$sc" skills install --agent detected
test ! -e "$env_home/.claude/skills"
test ! -e "$env_home/.config/sc/agent-skills-version"

printf '%s\n' '== config opt-out =='
config_home="$tmp/config-opt-out"
mkdir -p "$config_home/.claude" "$config_home/.config/sc"
printf '%s\n' 'install_agent_skills = false' >"$config_home/.config/sc/analyzer.toml"
HOME="$config_home" XDG_CONFIG_HOME="$config_home/.config" PATH="$bin" SCORECARD_NO_AGENT_SKILLS=0 "$sc" skills install --agent detected
test ! -e "$config_home/.claude/skills"
test ! -e "$config_home/.config/sc/agent-skills-version"

printf '%s\n' '== check does not create absent agent dirs =='
check_home="$tmp/check"
mkdir -p "$check_home"
set +e
HOME="$check_home" XDG_CONFIG_HOME="$check_home/.config" PATH="$bin" SCORECARD_NO_AGENT_SKILLS=0 "$sc" skills install --agent kiro --check >/dev/null
status=$?
set -e
test "$status" -eq 1
test ! -e "$check_home/.kiro"

printf '%s\n' '== no agents does not consume the version =='
empty_home="$tmp/no-agents"
mkdir -p "$empty_home"
HOME="$empty_home" XDG_CONFIG_HOME="$empty_home/.config" PATH="$tmp/empty-bin" SCORECARD_NO_AGENT_SKILLS=0 "$sc" skills install --agent detected >/dev/null
test ! -e "$empty_home/.config/sc/agent-skills-version"
mkdir -p "$empty_home/.claude"
HOME="$empty_home" XDG_CONFIG_HOME="$empty_home/.config" PATH="$tmp/empty-bin" SCORECARD_NO_AGENT_SKILLS=0 "$sc" skills install --agent detected
test -f "$empty_home/.claude/skills/scorecard/SKILL.md"

printf '%s\n' 'PASS: detected install, Codex destinations, version idempotency, preservation, opt-outs, and check mode'
