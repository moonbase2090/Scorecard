# LLM spec review

`--llm on` asks a model whether the code does what a spec says. It runs from `--spec`, or from `--intent` alone when there is no spec. Gaps are `spec.llm_gap` warnings, and the review never changes the exit status. It is off by default.

```bash
sc analyze . --spec TASK.md --llm on
```

`TASK.md` is any text that describes the change: a task, an issue, a design note. `--intent "…"` adds a one-line goal.

## Providers

The provider is set in `analyzer.toml` at the project root. Without a config, `sc` uses local Ollama.

### Ollama (default, local)

Nothing leaves your machine.

```bash
ollama pull qwen2.5-coder
ollama serve
```

```toml
[llm]
model = "qwen2.5-coder"               # default
endpoint = "http://127.0.0.1:11434/v1" # default
```

If `XAI_API_KEY` is set and `endpoint` is unchanged, `--llm on` calls xAI (`grok-4.5`) instead of Ollama. Unset it to stay local.

### OpenRouter or another OpenAI-compatible API

The spec and the file text the model asks for are sent to the provider.

```toml
[llm]
backend = "openai-compatible"
model = "openai/gpt-4o-mini"     # any model id the provider offers
# base_url = "https://openrouter.ai/api/v1"   # default; change for another provider
# api_key_env = "OPENROUTER_API_KEY"          # default
```

```bash
export OPENROUTER_API_KEY=sk-or-...
```

Without the key the review is skipped and the report says why:

```bash doctest
printf '[llm]\nbackend = "openai-compatible"\nmodel = "openai/gpt-4o-mini"\n' > analyzer.toml
printf 'Add the function `add` in `src/lib.rs`\n' > TASK.md
sc analyze . --spec TASK.md --llm on >/dev/null
jq -r '.findings[] | select(.engine == "llm") | .message' .sc/last-scorecard.json
```

prints `OPENROUTER_API_KEY is not set`.

Each review makes at most `max_tool_rounds` + 2 calls (default 38).

### Cursor

Runs `cursor-agent` in read-only ask mode. The spec and the files the agent reads are sent to Cursor.

```toml
[llm]
backend = "cursor"
model = "your-cursor-model"
```

All `[llm]` keys are in the [config reference](../reference/config.md#llm).
