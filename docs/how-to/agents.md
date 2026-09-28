# Agents and MCP

## Set up

```bash doctest
sc setup
```

`sc setup` does two things for the current user:

1. Installs the Scorecard skill, a short instruction file that tells an agent how to run `sc` and read its report, in `~/.claude/skills/scorecard`, `~/.cursor/skills/scorecard`, `~/.grok/skills/scorecard`, and `~/.agents/skills/scorecard`.
2. When `sc-mcp` is on `PATH`, registers it by absolute path as the MCP server `sc` in `~/.claude.json`, `~/.cursor/mcp.json`, and `~/.grok/config.toml`. Without `sc-mcp` it prints `mcp skipped`.

`sc-mcp` ships in every release archive and the macOS installer. From source: `cargo install --locked --path crates/sc-mcp`. Run `sc setup` again after installing or moving either binary, then reload MCP servers in the agent.

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
