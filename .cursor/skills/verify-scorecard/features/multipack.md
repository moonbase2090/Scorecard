# Multi-pack monorepo analysis

Use a fresh scratch repository with one static web root and one Go root. The
web root produces a real HTML finding; the unsupported Go analyzer contributes
a gate reason, proving both pack roots reached the combined report.

```bash
sc_multi="$sc_scratch/multi"
mkdir -p "$sc_multi/web-app" "$sc_multi/go-app"
printf '%s\n' '<html><body><p>Broken</p></body></html>' > "$sc_multi/web-app/index.html"
printf '%s\n' 'module example.com/go-app' '' 'go 1.23' > "$sc_multi/go-app/go.mod"
printf '%s\n' 'package main' 'func main() {}' > "$sc_multi/go-app/main.go"
printf 'pack = "multi"\n' > "$sc_config"
sc_run multipack 0 "$sc_multi" --fail-on none
python3 - "$sc_multi/.sc/last-scorecard.json" <<'CHECK'
import json, pathlib, sys
card = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert card['pack'] == 'multi'
assert any(f['file'] == 'web-app/index.html' for f in card['findings'])
assert any('go-app:' in (gate.get('reason') or '') for gate in card['gates'])
assert not (pathlib.Path(sys.argv[1]).parent.parent / 'web-app/.sc/last-scorecard.json').exists()
CHECK
```
