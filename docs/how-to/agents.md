# Agents and MCP

## Set up

```bash doctest
sc setup
```

`sc setup` does two things for the current user:

1. Installs the Scorecard skill in the legacy locations `~/.claude/skills/scorecard`, `~/.cursor/skills/scorecard`, `~/.grok/skills/scorecard`, and `~/.agents/skills/scorecard`. It leaves any existing skill file unchanged.
2. When `sc-mcp` is on `PATH`, registers it by absolute path as the MCP server `sc` in `~/.claude.json`, `~/.cursor/mcp.json`, and `~/.grok/config.toml`. Without `sc-mcp` it prints `mcp skipped`.

`sc-mcp` ships in every release archive and the macOS installer. From source: `cargo install --locked --path crates/sc-mcp`. Run `sc setup` again after installing or moving either binary, then reload MCP servers in the agent.

## Install the skill

`sc skills install` installs the embedded skill without changing MCP settings. By default it targets Codex (`~/.codex/skills/scorecard` and `~/.agents/skills/scorecard`), Claude Code (`~/.claude/skills/scorecard`), Cursor (`~/.cursor/skills/scorecard`), Kiro (`~/.kiro/skills/scorecard`), and Muse through `muse skills install`. `--agent codex` writes both Codex and shared locations.

```bash
sc skills install --agent codex
sc skills install --agent detected
sc skills install --agent all --check
```

`detected` includes an agent when its config directory already exists or its CLI is on `PATH`. It never creates a config directory for an agent that was not detected. Existing skill files that differ from the embedded copy are preserved; `--force` replaces them. `--check` performs no writes and exits 1 when a target is missing or differs.

The macOS package runs `sc skills install --agent detected` after creating the user config. It runs once for each Scorecard version and records the version in `~/.config/sc/agent-skills-version`. Installation errors are logged and do not fail the package installation. Automatic installation is enabled by default. Set `install_agent_skills = false` in `~/.config/sc/analyzer.toml` or set `SCORECARD_NO_AGENT_SKILLS=1` in the package installer environment to opt out. The same opt-outs apply when running `sc skills install --agent detected` directly; explicit agent targets remain available.

## MCP tools

`sc-mcp` speaks MCP over stdio.

| Tool | Does |
|---|---|
| `analyze_paths` | Runs `sc analyze` on the named files |
| `analyze_diff` | Runs `sc analyze --diff` |
| `list_findings` | Lists findings from `.sc/last-scorecard.json` |
| `explain` | Explains one finding from that report |

## Register by hand

Claude Code:

```bash
claude mcp add sc -- sc-mcp
```

Cursor, in `~/.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "sc": {"command": "sc-mcp", "args": []}
  }
}
```

## Without MCP

Any agent that can run commands can use `sc analyze . --format json`, or read `.sc/last-scorecard.json` after a run. Findings with disposition `fix` are the ones to act on; see [Reading the report](../report.md#dispositions).
