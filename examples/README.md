# Examples

## Generated and vendored files

Recursive scans honor root and nested `.gitignore` files. They also skip common build outputs, dependency trees, dot-directories, and source files marked with `@generated` or `Code generated ... DO NOT EDIT`.

Add project-specific paths to `[scope].exclude`. To inspect a built-in generated or vendored directory, add it to `[scope].include_generated`:

```toml
[scope]
exclude = ["src/generated/**"]
include_generated = ["build/**"]
```

If `.gitignore` excludes `build/`, remove that rule or add a matching negation. `scope.exclude` still wins. Reports list any generated or vendored paths that the scan included and suggest adding them to `scope.exclude`.

## Git status

A clean checkout stays clean after Scorecard writes its report. When the tree has changes, the human-readable reports name up to three paths. JSON lists every changed path:

```json
{
  "git": {
    "dirty": true,
    "dirty_paths": ["src/main.rs"]
  }
}
```

## Large files and NUL bytes
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

## PEM keys in source files

The secrets scan reports `secrets.private_key` when PEM markers and key material are split across string lines. It handles Go and Java concatenation, backtick strings, Python string prefixes, and YAML lists.

For example, the PEM body can appear in a Go string concatenation:

```go
key := "-----BEGIN RSA PRIVATE KEY-----\n" +
    "<base64 key data>\n" +
    "-----END RSA PRIVATE KEY-----\n"
```

It can also appear in a YAML list:

```yaml
private_key:
  - "-----BEGIN RSA PRIVATE KEY-----"
  - "<base64 key data>"
  - "-----END RSA PRIVATE KEY-----"
```

Run `sc analyze .` to report these as `secrets.private_key`. See the [rules reference](../docs/reference/rules.md#rules) for the finding and fix.

## Secrets

Run `sc analyze .` from the project root to check secrets. A Slack incoming
webhook reports as `secrets.slack_webhook`, a Stripe `rk_live_` key as
`secrets.stripe_key`, and an AWS Terraform provider `secret_key` as
`secrets.aws_secret_key`. These findings fail the secrets gate by default.
