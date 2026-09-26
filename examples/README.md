# Examples

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
