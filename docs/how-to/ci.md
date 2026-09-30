# Run sc in CI

## GitHub Actions

```yaml
name: scorecard
on: [pull_request]
permissions:
  contents: read
  security-events: write   # SARIF upload to code scanning
jobs:
  sc:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0     # --diff needs the base commit
      - uses: moonbase2090/Scorecard/action@v0.1.5
        with:
          diff: origin/${{ github.base_ref }}
```

The action installs `sc`, runs `sc analyze .`, and fails the job when a gate selected by `fail-on` fails. The report verdict remains `FAIL` for any enforced gate failure, even if `fail-on` lets the job exit 0. On public repositories the SARIF report appears in the pull request's code scanning alerts. Only `secrets.*` findings upload at error level; every other rule (test failures, missing coverage, lint, CRAP) uploads at warning, so quality signals never count as security vulnerabilities. With `format: html` or `all`, the report is uploaded as the `scorecard-report` artifact.

| Input | Default | Meaning |
|---|---|---|
| `fail-on` | `types,tests,crap,secrets,lint` | Gates whose failures fail the job. Empty exits 0 but does not hide failures from gates that remain enforced in the report. |
| `diff` | `""` | Base ref for `--diff`. Empty scores the whole tree. On a pull request, pass the base ref so the report covers changed code and a one-line count of the rest of the tree. |
| `format` | `sarif` | `json`, `md`, `sarif`, `html`, or `all` |
| `spec` | `""` | Path for `--spec` |
| `mutation` | `off` | `off`, `diff`, or `full` |
| `version` | the action's tag | `sc` release to install, such as `0.1.5` |

The runner needs the tools your pack uses (a Rust toolchain, Python with pytest, and so on); see [packs](../packs.md). A missing coverage tool does not fail the job, and the report says coverage was not measured.

## Any other CI

Download `sc` as in the [quickstart](../../README.md#quickstart), then:

```bash doctest
sc analyze . --format all --out sc-report --budget-seconds 600 >/dev/null
```

The exit status is the gate result (0 pass, 1 fail, 2 could not run). Keep `sc-report.html` as a build artifact and upload `sc-report.sarif` to any SARIF viewer. `--budget-seconds` raises the 120-second wall-clock budget for pack commands when the suite is slow.
