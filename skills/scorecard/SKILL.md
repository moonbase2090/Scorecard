---
name: scorecard
description: >
  Run the local Scorecard gate (command `sc`) on a project.
  Use when the user asks to check, score, lint, or gate code,
  or when code quality work needs a scorecard before it is called done.
---

# Scorecard

From the project root, run `sc analyze --format json`.

| Exit | Meaning |
| --- | --- |
| 0 | The default gates passed. |
| 1 | A gate failed. Fix findings whose `disposition` is `fix`, then run `sc` again. |
| 2 | A required tool did not run. Read the message and install that tool. |

A finding with `enforced: false` is reported and does not fail the run. The last report is `.sc/last-scorecard.json`. For a human-readable visual report, run `sc analyze --format html --out report.html` (self-contained, works from `file://`).

When the MCP server `sc` is connected, call `analyze_diff` for a change and `analyze_paths` for named files. Call `list_findings` and `explain` on that report.

Pass `--pack` when the tree has two language markers. Raise `--budget-seconds` when the test suite needs more than 120 seconds.
