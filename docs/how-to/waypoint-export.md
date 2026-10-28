# Export findings to Waypoint

`sc export --waypoint` reads a saved JSON report and creates or updates Waypoint tickets. It does not run analysis.

```sh
sc analyze . --format json --out .sc/last-scorecard.json
sc export --waypoint --project my-project --dry-run
```

The dry run lists the file/function groups it would reconcile and makes no network requests. To export, set a Waypoint API URL and bearer token in the environment:

```sh
export WAYPOINT_API_URL=https://waypoint.example
export WAYPOINT_API_TOKEN='your-token'
sc export --waypoint --project my-project
```

The token is read from `WAYPOINT_API_TOKEN` by default. Use `--token-env NAME` to choose another environment variable. Do not put the token in shell arguments or report files. `--agent-id` (or `WAYPOINT_AGENT_ID`, then `PMUX_AGENT`) sets the optional acting identity.

Findings are grouped by normalized relative file and symbol. The exporter creates one task ticket per group, identified by a stable hash label. It adds one comment per finding, containing the finding fingerprint, rule, severity, source location, message, and suggested action. Evidence is omitted. Re-running the same report detects existing fingerprints and leaves those comments untouched; changed findings are added to the same ticket. Ticket creation and comments are reconciled separately, so retrying after an interrupted run resumes without creating duplicate tickets or comments. This is retry-safe for sequential runs; do not run concurrent exports for the same project.

Absolute paths and paths containing parent traversal are rejected to avoid publishing local filesystem paths. Archived matching tickets are reported and left unchanged. Review the dry-run plan and destination project before the first live export.
