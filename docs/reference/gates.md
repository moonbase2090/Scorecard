# Gates

A gate is a pass or fail check. Enforced gates decide the exit status; advisory gates are reported only. `gates.fail_on` (or `--fail-on`) is the list of enforced gates.

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

When a function has no coverage record and no measured function fails, the `crap` gate is advisory for that run: the gate shows `advisory` and the reason says coverage was not measured. A measured function still fails the gate when it is over the CRAP threshold, or when its CC is at or above `gates.new_fn_untested_cc` with 0% coverage (`complexity.untested`), even if another function has no coverage record. The reason then also says some functions were not scored.

A function with no coverage record is not scored, so it cannot fail the gate itself. A file that the tests never load can have no record, depending on the pack's coverage tool.

To enforce an advisory gate, add it to the list:

```bash doctest
sc analyze . --fail-on types,tests,crap,secrets,lint,sca >/dev/null
```

or in `analyzer.toml`:

```toml
[gates]
fail_on = ["types", "tests", "crap", "secrets", "lint", "sca"]
```
