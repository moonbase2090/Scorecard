# Troubleshooting

Each section starts with what the report says. For a tool that is not installed, run `sc doctor` in the project: it lists every missing tool with its install command.

## `tests: pytest is not installed`

When the project has `uv.lock` or a `.venv` directory, `sc` runs `uv run --extra dev --with pytest-cov pytest`. If `uv` itself is not installed, the finding says `uv is not installed`. Run `sc doctor` to check the runner. If uv is missing, install it with `python3 -m pip install uv`, then create the environment once:

```bash
uv venv && uv pip install -e ".[dev]"
```

For projects without `uv.lock` or `.venv`, install pytest with `python3 -m pip install pytest`.

## Coverage not measured

The `crap` gate shows `advisory` with `some functions have no coverage record and were not scored`, and the worst-CRAP table is empty (`(none)`). A `coverage.missing` warning says coverage was not measured and how to collect it. Tests did not run, or the coverage tool for the pack is missing:

| Pack | Install |
|---|---|
| Rust | `rustup component add llvm-tools && cargo install cargo-llvm-cov` |
| Python | `python3 -m pip install pytest-cov` or `python3 -m pip install coverage`; uv supplies coverage tools automatically for uv projects. `sc doctor` checks these when a suite exists and coverage is enabled. |
| Node | `c8` |
| Go | nothing; uses `go test -coverprofile` |
| Bash | `kcov` |
| C++ | `lcov` |
| PHP | PHPUnit with `pcov` |

Until coverage runs, no CRAP scores are reported and nothing is treated as 0% coverage; the `crap` gate stays advisory.

## `types: compiler errors` with a rustup message

When `analyzer.toml` sets `toolchain`, Cargo check, test, coverage, and lint use that channel. When a `rust-toolchain.toml` or `rust-toolchain` is at or above the project, `sc` clears an inherited `RUSTUP_TOOLCHAIN` so rustup reads the file (including components). With no pin, an inherited `RUSTUP_TOOLCHAIN` is left alone. Install the channel with `rustup toolchain install`. The `scorecard-tools` Docker image only has stable; a non-stable `analyzer.toml` pin fails there until that channel is in the image.

`rustup could not choose a version of cargo to run` means no default Rust toolchain:

```bash
rustup default stable
```

## `ignoring N macOS metadata file(s)`

Copying a project through a non-Mac volume or archive leaves AppleDouble (`._*`) and `.DS_Store` files. `sc` skips them, but `pytest`, `cargo`, and other tools can read them and fail or time out. Delete them:

```bash
find . \( -name '._*' -o -name .DS_Store \) -type f -delete
```

## `several language packs match (rust, node)`

Exit 2 when more than one language marker matches and more than one of those languages has source files, or none of them do. A larger file count does not choose a pack. Pass `--pack rust`, or add `pack = "rust"` to `analyzer.toml`. Headers do not count as source and do not turn a Python project into an ambiguous tree. The terminal may cut this message short; `.sc/last-scorecard.json` has the full text. An empty tree says to set `pack` in `analyzer.toml` or pass `--pack`.

## The header says `dirty`

The report names up to three uncommitted or untracked paths below the Git status; the JSON lists them in `git.dirty_paths`. Scorecard's saved report, cache and coverage files under `.sc/`, and files selected by `--out`, are excluded. Add `.sc/` to `.gitignore` to keep Scorecard's generated files out of Git status too.

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

The import is not declared in `pyproject.toml`, `requirements.txt`, `setup.cfg`, or `setup.py` `install_requires`. This includes packages installed only as a dependency of another package, such as `botocore` through `boto3`. Declare it, or leave the warning: `sca` does not fail the run unless `fail_on` names it. A sentence in a docstring, `_typeshed`, and an import inside `try` / `except ImportError` are not this warning.

## Exit 2

`sc` could not run. The report says why:

| Cause | Fix |
|---|---|
| The path does not exist | Check the `PATH` argument |
| `config.invalid` | Fix `analyzer.toml` at the path in the message, for example an unknown gate in `fail_on` |
| Several language packs match | See [above](#several-language-packs-match-rust-node) |
| A required toolchain is missing | Install the compiler for the pack, or use the `scorecard-tools` image ([packs](packs.md)) |
| `<command> timed out after Ns` | The finding names the command, how long it ran, and the last stderr lines. Run that command in the project to see what it waits on, then raise `--budget-seconds` (default 120) if it only needs more time |
| `<command> did not run: earlier steps used the whole analysis budget` | The command never started. Check the report's runs for the earlier step that used the budget, then re-run with the larger `--budget-seconds` value suggested in the finding. No command duration or stderr is reported for an unstarted step |

## A config change has no effect

`sc` reads `analyzer.toml` only from the root of the analyzed directory, and ignores unknown keys. Check the file location and the key names in the [config reference](reference/config.md).
