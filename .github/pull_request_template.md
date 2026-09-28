Classification: leaf / trunk

## What this changes and why


## Proof

Provide relevant evidence: test output, a CI link, a screenshot, or a recording.

## How it was verified

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace --locked`
- [ ] `sc analyze .`

## Product quality bar ([REVIEW_POLICY.md](../REVIEW_POLICY.md#6-product-quality-bar))

- [ ] Common case works with no flags and no config
- [ ] Every new error or skip message says what, why, and the exact fix
- [ ] README, reference, and examples updated; `scripts/check-docs.py` passes
- [ ] Before and after terminal output (and report screenshots) from a real project run are above
- [ ] First-run path walked end to end, if this touches it
- [ ] No unmeasured number is shown as measured
- [ ] Smallest diff; no speculative features, unused options, or drive-by refactors
- [ ] No duplicated logic; touched dead code removed
- [ ] New flags or config keys justified; defaults preferred
- [ ] Docs tight and exact
- [ ] Every claim verified by a command that was run
- [ ] Tests assert real behavior and fail without the fix
- [ ] No new suppressions of `sc` findings
