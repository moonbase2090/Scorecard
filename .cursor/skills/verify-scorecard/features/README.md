# Scorecard verification map

Start with [Launch and Doctor](../SKILL.md). Commands run in the same Bash shell and use the run's copied web fixture. The seed map covers these CLI paths in order:

1. [Analysis and gate exits](analysis.md): passing input, failing input, report-only exit, persisted result, and an invalid path.
2. [Reports](reports.md): pretty output and JSON, Markdown, SARIF, and HTML files through `--format all --out`.
3. [Scope](scope.md): path lists, uncommitted Git changes against `HEAD`, and a missing diff base.

Capture every entry in a feature when that feature changes. A passing convenient format is not proof for another format. Keep action/output/exit evidence outside scratch state and confirm it survives Cleanup.

4. [Waypoint export](waypoint-export.md): dry-run planning, local HTTP protocol, and retry idempotency.

This initial map does not cover MCP requests, `sc setup`, user-config creation, agent installation, every language pack, mutation, or external LLM review. When touching those paths, extend the map and use the existing fixtures and protocol tests rather than treating this web smoke pass as complete coverage. User-global setup needs an isolated account/container; these recipes do not redirect the user's home or mutate live agent configuration. Consult [CLI reference](../../../../docs/reference/cli.md), [pack prerequisites](../../../../docs/packs.md), and [MCP documentation](../../../../docs/how-to/agents.md) for those branches.
