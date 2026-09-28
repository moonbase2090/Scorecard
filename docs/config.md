# Config

See `analyzer.toml.example`. Copy it to `analyzer.toml` at the root of the project you analyze, or to `~/.config/sc/analyzer.toml`. `sc` reads `analyzer.toml` from the root of the directory it analyzes (the path given to `sc analyze`, usually the repo root), then from `~/.config/sc/analyzer.toml`. It does not look in parent or sub-directories. `--config PATH` overrides both. `--fail-on` overrides `gates.fail_on`. CLI flags override the matching settings.

`secrets` matches a small token set (AWS access keys, GitHub tokens, Slack tokens, Stripe live keys, and private-key blocks). An undeclared dependency is a strongly advised warning (`sca.hallucinated_import`, disposition `ask`). The `sca` gate is `enforced: false` when it passes and when it fails, so it does not change the exit code under the default `--fail-on` list. `std`, `core`, `alloc`, `crate`, `self`, and `super` are allowed, as are modules declared in the crate (`mod x;`, `mod x {}`, file modules) and `extern crate` aliases. Python uses the same warning for an import that is not in `pyproject.toml`.

`--spec FILE` checks that paths named in the file exist and that `fn`, `struct`, `enum`, `trait`, `type`, and `const` names in the file are public items. Gaps fill `spec.gaps`. The `spec` gate fails when `--spec` is set or when `spec` is in `--fail-on` and the file is missing.

`--mutation diff` runs `cargo mutants --in-diff` against the same base as `--diff` (or `HEAD~1`). `--mutation full` runs the whole crate. Survivors are `mutation.survivor`. Timeouts are counted and are not kills. If `cargo-mutants` is missing, or the diff has more mutants than `mutation.max_mutants` (default 50), the engine is skipped with `engine.unavailable`. The mutation gate fails only when the mode is not `off`.

`--intent TEXT` is stored on the scorecard and sent with `--llm on`. It is the caller's goal. Scorecard does not read agent transcripts.

Each command is listed in `runs` with its exit code and duration. `cargo check`, coverage, and lint are recorded there too.

`commands.lint` defaults to `cargo clippy -- -D warnings`. A non-zero exit is `lint.failed` with disposition `fix`. A missing lint command is `engine.unavailable` with disposition `ask`. The `lint` gate fails the process only when `lint` is in `--fail-on`. Perf warnings stay disposition `ignore`.

`--llm on` sends the spec to an OpenAI-compatible `/chat/completions` endpoint. The built-in endpoint is `http://127.0.0.1:11434/v1` (local Ollama). That is the default backend (`[llm] backend = "ollama"`). If that endpoint is unchanged and `XAI_API_KEY` is set, the call uses SpaceXAI at `https://api.x.ai/v1` with model `grok-4.5`. The model may add `spec.llm_gap` warnings. It does not invent CRAP or mutation scores. A failed call skips the engine and does not fail the run. The loop allows 12 tool rounds and 2000 output tokens. Tools are `get_file`, `get_span`, `callers_of`, `tests_covering`, and `spec_section`.

`[llm] backend = "cursor"` is opt-in. It runs `cursor-agent` in ask mode (`--print --mode ask`) so the agent can read the repository and confirm a gap before reporting it. Ask mode is read-only: Scorecard does not pass `--force` or `--yolo`, and the prompt tells the agent not to edit files or run commands that change the tree. The spec and the files the agent reads are sent to Cursor. Set `[llm] model` to a `cursor-agent` model name. Local Ollama stays the default and does not contact Cursor.

`[llm] backend = "openai-compatible"` is opt-in. It sends the spec and the file text returned by those same tools to `base_url` (default `https://openrouter.ai/api/v1`). Set `model` to a model id that provider offers. The API key is read from the environment variable named by `api_key_env` (default `OPENROUTER_API_KEY`). The key is not written into config or into the finding text. Local Ollama stays the default and does not contact that provider.
