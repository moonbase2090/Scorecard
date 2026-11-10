# Badge

Read a saved report as a Shields.io badge. A finished run shows a letter grade. An analyzer error shows `error` and no letter.

## Sub-features

- `letter`: a passing web report badges as a letter grade out of 100.
- `error`: a missing project is marked `analyzer_error`, and the badge message is `error`.

## How to get to it (user POV)

Run `sc analyze PATH`, then `sc badge` in that project. `sc badge --input FILE` reads a different saved report.

## Driving it with sc_run

Preconditions: Launch and Doctor completed. The copied `site` is still the original web fixture.

```bash
sc_run letter 0 "$sc_scratch/site" --format json
(
  cd "$sc_scratch/site"
  env SCORECARD_NO_AGENT_SKILLS=1 "$sc_binary" badge > "$sc_evidence/letter.badge"
)
python3 - <<'CHECK'
import json, os, pathlib, re
p = pathlib.Path(os.environ['sc_evidence'])
card = json.loads((p / 'letter.stdout').read_text())
assert card['verdict'] == 'pass'
assert 'analyzer_error' not in card
badge = json.loads((p / 'letter.badge').read_text())
assert badge['schemaVersion'] == 1
assert badge['label'] == 'scorecard'
assert re.fullmatch(r'[ABCDF] \([0-9]+/100\)', badge['message'])
CHECK
sc_run missing 2 "$sc_scratch/does-not-exist" --format json
cp "$sc_evidence/missing.stdout" "$sc_evidence/missing-report.json"
python3 - <<'CHECK'
import json, os, pathlib, subprocess
evidence = pathlib.Path(os.environ['sc_evidence'])
card = json.loads((evidence / 'missing.stdout').read_text())
assert card['analyzer_error'] is True
assert card['verdict'] == 'fail'
badge = subprocess.run(
    [os.environ['sc_binary'], 'badge', '--input', str(evidence / 'missing-report.json')],
    check=True, capture_output=True, text=True,
)
body = json.loads(badge.stdout)
assert body == {
    'schemaVersion': 1,
    'label': 'scorecard',
    'message': 'error',
    'color': 'lightgrey',
}
assert 'A (' not in body['message']
CHECK
```

## Gotchas

`sc_run` always invokes `analyze`. The badge command is a separate process. A missing path does not create a project directory, so copy that run's JSON and pass it to `sc badge --input`. A previous saved report in another directory is not this run.
