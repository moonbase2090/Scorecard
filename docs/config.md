# Config

See `analyzer.toml.example`. Copy it to `analyzer.toml` or `~/.config/sc/analyzer.toml`. `--fail-on` overrides `gates.fail_on`. CLI flags override the matching settings.

`secrets` matches a small token set (AWS access keys, GitHub tokens, Slack tokens, Stripe live keys, and private-key blocks). An undeclared dependency is a strongly advised warning (`sca.hallucinated_import`, disposition `ask`). The `sca` gate is `enforced: false` when it passes and when it fails, so it does not change the exit code under the default `--fail-on` list. `std`, `core`, `alloc`, `crate`, `self`, and `super` are allowed, as are modules declared in the crate (`mod x;`, `mod x {}`, file modules) and `extern crate` aliases. Python uses the same warning for an import that is not in `pyproject.toml`.

`--spec FILE` checks that paths named in the file exist and that `fn`, `struct`, `enum`, `trait`, `type`, and `const` names in the file are public items. Gaps fill `spec.gaps`. The `spec` gate fails when `--spec` is set or when `spec` is in `--fail-on` and the file is missing.

`--mutation diff` runs `cargo mutants --in-diff` against the same base as `--diff` (or `HEAD~1`). `--mutation full` runs the whole crate. Survivors are `mutation.survivor`. Timeouts are counted and are not kills. If `cargo-mutants` is missing, or the diff has more mutants than `mutation.max_mutants` (default 50), the engine is skipped with `engine.unavailable`. The mutation gate fails only when the mode is not `off`.

`--intent TEXT` is stored on the scorecard and sent with `--llm on`. It is the caller's goal. Scorecard does not read agent transcripts.

Each command is listed in `runs` with its exit code and duration. `cargo check`, coverage, and lint are recorded there too.

`commands.lint` defaults to `cargo clippy -- -D warnings`. A non-zero exit is `lint.failed` with disposition `fix`. A missing lint command is `engine.unavailable` with disposition `ask`. The `lint` gate fails the process only when `lint` is in `--fail-on`. Perf warnings stay disposition `ignore`.

`--llm on` sends the spec to an OpenAI-compatible `/chat/completions` endpoint. The built-in endpoint is `http://127.0.0.1:11434/v1`. If that endpoint is unchanged and `XAI_API_KEY` is set, the call uses SpaceXAI at `https://api.x.ai/v1` with model `grok-4.5`. The model may add `spec.llm_gap` warnings. It does not invent CRAP or mutation scores. A failed call skips the engine and does not fail the run. The loop allows 12 tool rounds and 2000 output tokens. Tools are `get_file`, `get_span`, `callers_of`, `tests_covering`, and `spec_section`.
