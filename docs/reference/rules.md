# Rules

Every finding has a rule id. Severity `error` counts against the gate; `warning` is reported only. The disposition (`fix`, `ask`, `ignore`) is explained in [Reading the report](../report.md#dispositions).

| Rule | Severity | Disposition | Means | Fix |
|---|---|---|---|---|
| `compile.error` | error | fix | The compiler reported an error. | Fix the error at the location shown. A rustup message here means no default toolchain: run `rustup default stable`. |
| `compile.failed` | error | fix | The build or syntax check failed. | Run the command from the finding (also in `runs` in the JSON) and fix what it reports. |
| `test.failed` | error | fix | A test failed. | Fix the test or the behavior it checks. |
| `crap.over_threshold` | error, or warning when coverage was not measured | fix | A function's CRAP score is over `gates.crap_threshold`. | Add tests that cover its branches, or split it. |
| `complexity.untested` | error, or warning when coverage was not measured | fix | A function at or above `gates.new_fn_untested_cc` complexity has 0% coverage. | Add tests for it, or split it. |
| `coverage.missing` | warning | ask | Coverage was not measured for some functions, usually because tests did not run or the coverage tool is missing. | Install the coverage tool for your pack (see [troubleshooting](../troubleshooting.md#coverage-not-measured)). |
| `coverage.unmatched` | warning | ask | Coverage ran, but some functions have no coverage record. | Check that those functions are built into the test run. `evidence.functions` lists them. |
| `lint.failed` | error | fix | The linter reported problems. A Clippy finding names the first diagnostic's file, line, and lint. | Run the lint command from the finding and fix what it reports. |
| `secrets.aws_access_key` | error | fix | An AWS access key id is in the tree. | Remove it, rotate the key, and rewrite history if it was pushed. |
| `secrets.aws_secret_key` | error | fix | An AWS secret access key is in the tree. | Remove it, rotate the key, and rewrite history if it was pushed. |
| `secrets.github_token` | error | fix | A GitHub token is in the tree. | Remove it and revoke the token. |
| `secrets.slack_token` | error | fix | A Slack token is in the tree. | Remove it and revoke the token. |
| `secrets.stripe_key` | error | fix | A Stripe live key is in the tree. | Remove it and roll the key. |
| `secrets.private_key` | error | fix | A private-key block is in the tree. | Remove it and replace the key. |
| `secrets.unreadable` | error | fix | A file the secrets scan should read could not be opened, so the gate does not pass. | Restore read access, or exclude the path with `exclude = ["path"]` under `[scope]` in `analyzer.toml`. |
| `secrets.partial` | error | fix | A text file over 64 MiB was not read, so the scan was partial and the gate does not pass. A lockfile over 1 MiB is still scanned. A NUL or a gitignore entry skips a larger build output instead. | Exclude that path with `exclude = ["path"]` under `[scope]` in `analyzer.toml`, or move the secret out of the large file. |
| `sca.hallucinated_import` | warning | ask | An import is not a local module, not installed, and not on the package index. | Remove the import or correct the module name. |
| `sca.undeclared_dependency` | warning | ask | An installed or published package is imported and is not declared. | Add it to the manifest (`Cargo.toml`, `pyproject.toml`, and so on). |
| `sca.import_unresolved` | warning | ask | An import is not local and not installed, and the package index was not checked. | Add the package to the manifest or remove the import. |
| `spec.missing_file` | error | fix | A file named in `--spec` does not exist. | Add the file, or fix the path in the spec. |
| `spec.missing_item` | error | fix | A public item named in `--spec` does not exist. | Add the item, or fix the spec. |
| `spec.llm_gap` | warning | ask | The LLM review found a gap between the spec and the code. | Update the code or the spec. |
| `mutation.survivor` | error | fix | A mutant survived the tests. | Add a test that fails for that mutant. |
| `html.parse` | error | fix | The page does not parse cleanly. | Fix the markup at the location shown. |
| `html.doctype` | error | fix | The page has no `<!doctype html>`. | Add it as the first line. |
| `html.viewport` | error | fix | The page has no viewport `meta` tag. | Add `<meta name="viewport" content="width=device-width, initial-scale=1">`. |
| `html.misnested` | error | fix | Tags close in the wrong order. | Close the inner tag first. |
| `html.unclosed` | error | fix | A tag is never closed. | Close the tag. |
| `links.missing` | warning | ask | A link or `src` points at a file that is not in the tree. | Point it at an existing file, or remove it. |
| `engine.unavailable` | warning (error when a required toolchain is missing) | ask | A check could not run; the message says which tool or setting is missing. | See [troubleshooting](../troubleshooting.md). |
| `config.invalid` | error | fix | `analyzer.toml` could not be read, or names an unknown gate. `sc` exits 2. | Fix the file at the path in the message. |

Accessibility rule ids (`img-alt`, `label`, `contrast`, and others) are listed in [accessibility](../a11y.md).
