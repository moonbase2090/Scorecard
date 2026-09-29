# Examples

## Large files and NUL bytes

The secrets scan checks files up to 64 MiB, including large files with NUL bytes. Gitignored large files are skipped. A non-ignored file over 64 MiB produces a `secrets.partial` finding because the scan cannot read the whole file.

If a generated file should not be scanned, exclude its path in `analyzer.toml`:

```toml
[scope]
exclude = ["assets/generated.bin"]
```

The [secrets rules](../docs/reference/rules.md#rules) reference describes `secrets.partial`.

## Python source layout

For a project with `src/my_package/` and tests that import `my_package`, `sc analyze .` puts `src/` first on `PYTHONPATH` while it runs pytest and collects coverage. Coverage then refers to the package files in the checkout, even when another copy is installed.

## Passing fixture

From a checkout of this repository, with `sc` on `PATH`:

```bash
sc analyze testdata/good_crate --format md
```

That fixture typechecks and its tests pass. The [readme sample](../README.md#sample) is the verbatim Markdown from that command on commit `0afc676`.

## Failing fixture

```bash
sc analyze testdata/failing_test --format md
```

That fixture has a broken test, so the verdict is `fail`, the report
lists a `test.failed` rule, and the process exits 1. Gate failures
always mean a nonzero exit; warnings (like the undeclared-dependency
advisory) do not fail the process on their own.

## SARIF levels

```bash
sc analyze testdata/failing_test --format sarif | jq -r '.runs[0].results[] | "\(.ruleId) | \(.level)"'
```

```text
coverage.missing | warning
test.failed | warning
```

The failing test is level `warning` in SARIF, so GitHub code scanning does not count it as a security vulnerability. Only `secrets.*` findings are level `error`. The JSON report still gives `test.failed` severity `error`, and the tests gate still fails the run.
