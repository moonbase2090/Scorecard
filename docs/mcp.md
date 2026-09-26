# MCP

`sc-mcp` speaks MCP over stdio. It serves four tools: `analyze_paths`, `analyze_diff`, `explain`, and `list_findings`. `explain` and `list_findings` read `.sc/last-scorecard.json` from the last analyze. Parse results are cached in `.sc/cache/parse-v1.json`.

`sc setup` makes that server visible to agents on this computer. It writes the skill to `~/.grok/skills/scorecard`, `~/.claude/skills/scorecard`, `~/.cursor/skills/scorecard`, and `~/.agents/skills/scorecard`. It registers `sc-mcp` in `~/.grok/config.toml`, `~/.cursor/mcp.json`, and `~/.claude.json` when `sc-mcp` is on `PATH`. Run it again after you install or move the binary. Reload MCP servers in the agent after that.
