# HTML report

`sc analyze --format html` writes one HTML file. The CSS is inline. The page does not load fonts, scripts, or images from the network, so it opens from `file://` and can be stored as a CI artifact.

```bash
sc analyze testdata/good_crate --format html --out scorecard.html
```

That command writes `scorecard.html` and prints the same HTML on stdout.

`--format all` still prints JSON, then Markdown, on stdout. With `--out`, it writes four siblings next to the path you give:

```bash
sc analyze testdata/good_crate --format all --out scorecard.html
```

| File | Contents |
|---|---|
| `scorecard.json` | JSON scorecard |
| `scorecard.md` | Markdown scorecard |
| `scorecard.sarif` | SARIF 2.1.0 |
| `scorecard.html` | This HTML report |

If `--out` does not end in `.html`, the HTML sibling uses the `.html` extension.

## Sections

The page is the report for one run, top to bottom:

1. **Header.** Verdict pill, repository, pack, scope, and the scorecard id. When no gate is enforced (an empty `--fail-on`) and gates fail, the pill is a neutral `REPORT ONLY` with the failing count instead of a green pass, so report-only runs never read as clean. The flow strip, the page title, the markdown verdict line, and the terminal banner say report only too. When the enforced gates pass and only advisory gates such as `sca` fail, the pill stays `PASS` with a note such as `1 advisory gate failing`. Under that: git commit (short SHA, full SHA on hover) and clean or dirty, test selection, engines that ran, engines that were skipped, and the paths in scope. Each summary cell clips and wraps inside its column, so a long SHA cannot paint over the tests column. Tree scope (or more than 10 paths) collapses behind a count with a `<details>` full list; paths wrap after `/` so lines never split mid-segment. The git probe runs before the engines, so files sc itself creates never flag the tree dirty.
2. **Flow.** The pipeline strip: scope, pack, engines, gates, verdict, with an arrow between each box. On `testdata/good_crate` the boxes read `tree · 1 path`, `rust`, `9 run · 3 skipped`, `6/6 pass`, and `pass`. The scope box counts paths in scope; the engines box counts skipped engines. The skipped engines on that run are spec, mutation, and llm.
3. **Scores.** correctness, efficiency, maintainability, security, and a11y, then the metric tiles (lines and files changed, coverage, CRAP max, functions over the threshold, undeclared deps). When the coverage engine did not run, the coverage tile reads `—` / `coverage not measured` instead of `0%`. The undeclared-deps tile is `metrics.undeclared_dependencies`. A hallucinated-imports tile appears only when `metrics.hallucinated_imports` is greater than zero.
4. **Gates.** Each gate, pass or fail, and whether it is enforced. Enforced reflects the run's `--fail-on` set: a failing gate outside `--fail-on` reads "reported only". A gate the pack does not provide renders as skipped and stays out of the flow strip's pass ratio. `sca` is reported only. It does not change the exit code.
5. **Worst CRAP.** Up to the functions in the reporting window, with the threshold from config. The coverage column shows a bar and the percentage. When coverage was not measured, each row says `not measured` and a note above the table says the CRAP numbers assume 0% coverage and are an upper bound. The markdown table and the terminal (`--`) say the same. In a `--diff` run the table covers changed functions only, and a line above it says so: `diff scope: 4 functions over threshold in this diff. The tree-wide count is on the latest push run of main.` The base comes from the scorecard's `scope.base`. A commit-ish base such as `HEAD~1` points to a tree-scope run instead. The markdown report prints the same line, and the terminal prints a short form.
6. **Findings.** The total count, then one collapsible group per rule with its count and error and warning pills. Groups that hold errors come first, smallest first, so a single `test.failed` is not buried under many CRAP findings. Error groups of 10 or fewer start open. Each group shows at most 50 cards and says how many more are in the JSON and SARIF reports. A `complexity.untested` finding for a function that also has a `crap.over_threshold` finding is folded into that card, with a note giving the count. Accessibility findings are listed under Accessibility, and a line here points to them. Repo-level findings (file `.`) have no location line. A finding that carries the raw command output in `evidence.log` (lint and test failures) shows its one-line message first, with the log behind a `full output` disclosure. The markdown report uses a `<details>` block for the same log. A finding that lists functions in `evidence.functions` (such as `coverage.unmatched`) shows up to 20 of them as `file:line · symbol`, then how many more, taking the total from `evidence.unmatched` when the engine capped the list. The list is skipped when the finding already names its single function. A clean run says `None. Clean gate.`
7. **LLM.** Present only when `--llm on` ran or was skipped after being turned on. A run shows backend, model, rounds, and verdict when those were recorded, plus up to 10 notes. A skip states what happened and the command or config line that changes it. A default run has no llm section. The terminal and the markdown report print the same block. The JSON field `llm` is omitted when llm is off. Missing rounds or verdict are left out rather than shown as zero or `no gaps`.
8. **Mutation and spec.** Status of the mutation run and whether a spec file was requested.
9. **Runs.** Engine, command, exit code, and duration in milliseconds. A long command scrolls inside its cell, so the exit code and duration columns stay visible.
10. **Footer.** `generated by sc analyze --format html`, the scorecard id, and the schema version.

The screenshot in the [readme](../README.md#html-report) is this page for `testdata/good_crate`, opened from the file written by the command above.
