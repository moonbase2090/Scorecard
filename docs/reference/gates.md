# Gates

A gate is a pass or fail check. Any failed enforced gate makes the report verdict `FAIL`; advisory gates are reported without affecting the verdict. `gates.fail_on` (or `--fail-on`) selects which enforced gate failures set process exit 1. Naming an advisory gate can enforce it; leaving an already enforced gate out of the exit list does not hide its failure from the report.

| Gate | Enforced by default | Fails when | Rules |
|---|---|---|---|
| `types` | yes | The project does not build or type-check | `compile.error`, `compile.failed` |
| `tests` | yes | A test fails | `test.failed` |
| `crap` | yes | A function's CRAP score is over `gates.crap_threshold`, or a complex function has 0% coverage | `crap.over_threshold`, `complexity.untested` |
| `secrets` | yes | A token or private key is in the tree | `secrets.*` |
| `lint` | yes | The linter reports problems | `lint.failed` |
| `sca` | no | An import is undeclared, hallucinated, or not classified | `sca.undeclared_dependency`, `sca.hallucinated_import`, `sca.import_unresolved` |
| `spec` | only with `--spec` | A file or public item named in the spec is missing | `spec.missing_file`, `spec.missing_item`, `spec.llm_gap` |
| `mutation` | only with `--mutation` | A mutant survives the tests | `mutation.survivor` |
| `html` | yes, web pack | The HTML does not parse cleanly | `html.*` |
| `links` | no | A link points at a missing file in the tree | `links.missing` |
| `a11y` | no | A static accessibility check fails | see [accessibility](../a11y.md) |

When a function has no coverage record and no measured function fails, the `crap` gate is advisory for that run: the gate shows `advisory` and the reason says some functions have no coverage record and were not scored. A measured function still fails the gate when it is over the CRAP threshold, or when its CC is at or above `gates.new_fn_untested_cc` with 0% coverage (`complexity.untested`), even if another function has no coverage record.

A function with no coverage record is not scored, so it cannot fail the gate itself. Node and Python record a file the tests never load at 0%. With other packs' coverage tools, such a file can have no record.

To enforce an advisory gate, add it to the list:

```bash doctest
sc analyze . --fail-on types,tests,crap,secrets,lint,sca >/dev/null
```

or in `analyzer.toml`:

```toml
[gates]
fail_on = ["types", "tests", "crap", "secrets", "lint", "sca"]
```
