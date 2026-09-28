# Troubleshooting

Each section starts with what the report says.

## `tests: pytest is not installed`

When the project has `uv.lock` or a `.venv` directory, `sc` runs `uv run --extra dev --with pytest-cov pytest`. Otherwise it runs `python3 -m pytest` from `PATH`. Create the environment once:

```bash
uv venv && uv pip install -e ".[dev]"
```

or activate your own virtualenv, with pytest installed, before running `sc`.

## Coverage not measured

The `crap` gate shows `advisory` with `coverage was not measured`, and COV shows `--`. Tests did not run, or the coverage tool for the pack is missing:

| Pack | Install |
|---|---|
| Rust | `rustup component add llvm-tools && cargo install cargo-llvm-cov` |
| Python | `pytest-cov` (added automatically when `sc` runs tests through `uv`) |
| Node | `c8` |
| Go | nothing; uses `go test -coverprofile` |
| Bash | `kcov` |
| C++ | `lcov` |
| PHP | PHPUnit with `pcov` |

Until coverage runs, CRAP scores are an upper bound and CRAP failures do not fail the run.

## `types: compiler errors` with a rustup message

`rustup could not choose a version of cargo to run` means no default Rust toolchain:

```bash
rustup default stable
```

## `several language packs match (rust, node)`

Exit 2 when the matching languages have the same amount of source, such as one Rust file and one JavaScript file, or one Python file and one C header. Pass `--pack rust`, or add `pack = "rust"` to `analyzer.toml`. If one language has more source files, that pack is used and this error does not happen. Headers do not count. The terminal may cut this message short; `.sc/last-scorecard.json` has the full text. An empty tree says to set `pack` in `analyzer.toml` or pass `--pack`.

## The header says `dirty`

The tree has uncommitted or untracked files. After the first run this includes `.sc/`, where `sc` keeps its report. Add `.sc/` to `.gitignore`.

## `N more, see --out report`

The terminal lists the first findings only. All of them are in `.sc/last-scorecard.json`, or in the file written by `--out`. `--format html --out report.html` gives a browsable page.

## LLM review skipped

The `llm` engine is listed as skipped with an `engine.unavailable` finding that gives the reason:

| Reason | Fix |
|---|---|
| `llm is on, and no readable --spec file was given` | Pass `--spec FILE` |
| `OPENROUTER_API_KEY is not set` | `export OPENROUTER_API_KEY=...`, or set `llm.api_key_env` |

With the default Ollama backend, start `ollama serve` first. On the Python pack, `--spec` and `--llm` are currently not run and no finding is written. See [LLM providers](how-to/llm.md).

## `sca.hallucinated_import` for a package I use

The import is not declared in the manifest. This includes packages installed only as a dependency of another package, such as `botocore` through `boto3`. Declare it, or leave the warning: `sca` does not fail the run unless `fail_on` names it.

## Exit 2

`sc` could not run. The report says why:

| Cause | Fix |
|---|---|
| The path does not exist | Check the `PATH` argument |
| `config.invalid` | Fix `analyzer.toml` at the path in the message, for example an unknown gate in `fail_on` |
| Several language packs match | See [above](#several-language-packs-match-rust-node) |
| A required toolchain is missing | Install the compiler for the pack, or use the `scorecard-tools` image ([packs](packs.md)) |
| Compile or tests timed out | Raise `--budget-seconds` (default 120) |

## A config change has no effect

`sc` reads `analyzer.toml` only from the root of the analyzed directory, and ignores unknown keys. Check the file location and the key names in the [config reference](reference/config.md).
