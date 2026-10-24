# Reports

Read a terminal report or save portable reports without confusing the stdout summary with the full file contents.

## Sub-features

- `pretty`: explicit pretty format prints a passing terminal report.
- `all-files`: `--format all --out` writes four report files.
- `semantic-result`: JSON and SARIF parse; the report states a passing verdict.

## How to get to it (user POV)

Use `sc analyze PATH --format pretty`, or `sc analyze PATH --format all --out report.json`. Individual `json`, `md`, `sarif`, and `html` formats also exist; exercise them separately when their dispatch changes.

## Driving it with sc_run

Preconditions: Launch and Doctor completed; use the original valid `site` fixture.

```bash
sc_run pretty 0 "$sc_scratch/site" --format pretty
sc_run reports 0 "$sc_scratch/site" --format all --out "$sc_evidence/report.json"
python3 - <<'CHECK'
import json, os, pathlib
p = pathlib.Path(os.environ['sc_evidence'])
assert 'PASS' in (p / 'pretty.stdout').read_text()
for suffix in ['json', 'md', 'sarif', 'html']:
    assert (p / ('report.' + suffix)).stat().st_size > 0
assert json.loads((p / 'report.json').read_text())['verdict'] == 'pass'
sarif = json.loads((p / 'report.sarif').read_text())
assert sarif['version'] == '2.1.0'
assert sarif['runs'][0]['properties']['scorecardVerdict'] == 'pass'
assert 'PASS' in (p / 'reports.stdout').read_text()
assert str(p / 'report.json') in (p / 'reports.stdout').read_text()
assert sarif['runs'][0]['tool']['driver']['name'] == 'sc'
assert '**Verdict:** pass' in (p / 'report.md').read_text()
assert '<html' in (p / 'report.html').read_text().lower()
CHECK
```

Open `report.html` in a browser when report rendering changes; capture a screenshot of the affected state along with these CLI artifacts. File existence and HTML syntax alone do not prove the visual layout.

## Gotchas

`--out` prints a short stdout summary, including when redirected. Parse `report.json`, not that summary. These explicit-format commands do not prove automatic TTY format selection, color, or interactive progress; use a fresh PTY for those paths.
