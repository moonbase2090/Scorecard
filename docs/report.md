# Reading the report

`sc analyze` prints the terminal layout when stdout is a terminal and JSON otherwise. `--format` picks one explicitly; see [CLI](reference/cli.md#output). Every run also saves the JSON to `.sc/last-scorecard.json`.

## Terminal layout

```text
sc 0.1.3  .  python  99b879c dirty  scope tree
```

The header: `sc` version, analyzed path, [pack](packs.md), git commit, `clean` or `dirty`, and scope. Scope is `tree` (everything), `diff` (`--diff`), or `paths` (`--paths`).

```text
PASS  2 advisory gates failing
```

The verdict. `PASS` means every enforced gate passed. Advisory gates can still fail under a `PASS`; the note counts them. `FAIL` means an enforced gate failed and the exit status is 1.

```text
gates
  ✓  types            enforced
  ✗  tests            enforced  pytest is not installed
  ✗  sca              advisory  16 undeclared dependencies (advisory; does not fail the process)
```

One row per gate: result, `enforced` or `advisory`, and the reason when it did not pass. Only enforced gates change the exit status. [Gates](reference/gates.md) lists them and how to enforce an advisory one.

```text
scores
  correctness      1.00  [##########]
  maintainability  0.00  [..........]
  security         0.20  [##........]
```

Each score starts at 1.00. Every error finding in that area subtracts 0.25 and every warning 0.05, down to 0. Scores are a summary; the gates decide the verdict.

| Score | Lowered by findings from |
|---|---|
| correctness | compile, tests, lint, html, config |
| efficiency | perf |
| security | secrets, sca (undeclared dependencies) |
| a11y | a11y |
| maintainability | everything else: complexity, coverage, CRAP, mutation, spec, links |

```text
worst crap  threshold 30
  CRAP   CC   COV  SYMBOL            LOCATION
  75.7    9    6%  Config.from_ssm   src/vectorvault/config.py:56
```

The functions with the highest [CRAP](crap.md) score: cyclomatic complexity (CC) weighted by missing test coverage (COV). A score above the threshold fails the `crap` gate. `--` in the COV column means coverage was not measured for that function, and its CRAP score assumes no coverage.

```text
findings
error
  crap.over_threshold
  src/vectorvault/config.py:56  Config.from_ssm
  Add tests covering branches or split the function
```

Each finding: severity, [rule id](reference/rules.md), location, and a suggested fix. The terminal shows the first findings and a count of the rest; the full list is in `.sc/last-scorecard.json` and in any `--out` report.

```text
engines run: compile, tests, crap, sca, secrets, lint
engines skipped: complexity, mutation, llm
exit 0: gates passed
```

Which checks ran, which did not, and the exit status. An engine is skipped when its tool is missing or when it is off by default (`mutation`, `llm`, `spec`). A skip caused by a missing tool also appears as an `engine.unavailable` finding with the reason.

## Dispositions

Each JSON finding has a `disposition` that tells an agent what to do:

| Disposition | Meaning |
|---|---|
| `fix` | A real problem. Change the code. |
| `ask` | Needs a human decision, such as installing a tool or declaring a dependency. |
| `ignore` | Informational, such as performance hints. |

## JSON

Top-level fields of `.sc/last-scorecard.json` and `--format json`:

| Field | Contents |
|---|---|
| `verdict` | `pass` or `fail` |
| `pack`, `repo`, `git`, `scope` | What was analyzed |
| `gates` | `id`, `pass`, `enforced`, and `reason` per gate |
| `scores` | The score rows above |
| `metrics` | Changed lines and files, `coverage_changed`, `crap_max`, `crap_over_threshold`, `hallucinated_imports` |
| `crap` | Threshold and worst functions with `cc`, `coverage`, and `crap` |
| `findings` | `rule`, `engine`, `severity`, `file`, `span`, `symbol`, `message`, `suggested_action`, `disposition`, `evidence` |
| `engines_run`, `engines_skipped` | As in the terminal footer |
| `runs` | Every command `sc` ran, with exit code and duration |

List the findings an agent should fix:

```bash doctest
sc analyze . >/dev/null
jq -r '.findings[] | select(.disposition == "fix") | "\(.rule) \(.file) \(.message)"' .sc/last-scorecard.json
```

## Other formats

```bash doctest
sc analyze . --format all --out sc-report >/dev/null
ls sc-report.json sc-report.md sc-report.sarif sc-report.html
```

`--format all --out NAME` writes JSON, Markdown, SARIF, and a self-contained HTML page next to `NAME`. The HTML page is described in [HTML report](html-report.md). SARIF is for GitHub code scanning; see [CI](how-to/ci.md).

## The `.sc/` directory

| Path | Contents |
|---|---|
| `.sc/last-scorecard.json` | The last report. `sc-mcp` reads it. |
| `.sc/cache/` | Parse cache |
| `.sc/coverage/` | Coverage reports written by the test runs |

It is safe to delete. Add `.sc/` to `.gitignore`.
