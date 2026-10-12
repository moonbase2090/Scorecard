# Analysis and gate exits

Analyze a project and distinguish a clean report, a failed enforced gate, report-only exit behavior, and an analyzer error.

## Sub-features

- `clean`: valid web input passes its HTML gate.
- `failed`: removing the document type produces `html.doctype` and exit 1.
- `report-only`: `--fail-on none` returns exit 0 while preserving the failed verdict.
- `saved`: the latest report is written under the analyzed project.
- `invalid-path`: a missing project exits 2.

## How to get to it (user POV)

Run `sc analyze PATH --format json`. Choose `--fail-on none` to report failures without failing the process. An explicit `--config FILE` selects the run's local configuration.

## Driving it with sc_run

Preconditions: Launch and Doctor completed; the copied `site` still contains its original `index.html`.

```bash
sc_run clean 0 "$sc_scratch/site" --format json
cp "$sc_scratch/site/.sc/last-scorecard.json" "$sc_evidence/clean.saved.json"
cp "$sc_scratch/site/index.html" "$sc_scratch/index.original"
sed '1d' "$sc_scratch/index.original" > "$sc_scratch/site/index.html"
sc_run failed 1 "$sc_scratch/site" --format json
sc_run report-only 0 "$sc_scratch/site" --format json --fail-on none
cp "$sc_scratch/site/.sc/last-scorecard.json" "$sc_evidence/report-only.saved.json"
sc_run missing 2 "$sc_scratch/does-not-exist" --format json
mv "$sc_scratch/index.original" "$sc_scratch/site/index.html"
python3 - <<'CHECK'
import json, os, pathlib
p = pathlib.Path(os.environ['sc_evidence'])
clean = json.loads((p / 'clean.stdout').read_text())
assert clean['verdict'] == 'pass'
assert any(g['id'] == 'html' and g['pass'] and g['enforced'] for g in clean['gates'])
assert json.loads((p / 'clean.saved.json').read_text()) == clean
for name in ['failed', 'report-only']:
    card = json.loads((p / (name + '.stdout')).read_text())
    assert card['verdict'] == 'fail'
    assert any(f['rule'] == 'html.doctype' for f in card['findings'])
    assert any(g['id'] == 'html' and not g['pass'] and g['enforced'] for g in card['gates'])
assert json.loads((p / 'report-only.saved.json').read_text()) == json.loads((p / 'report-only.stdout').read_text())
assert json.loads((p / 'report-only.stdout').read_text())['fail_on'] == []
missing = json.loads((p / 'missing.stdout').read_text())
assert missing['verdict'] == 'fail'
assert any(f['rule'] == 'engine.unavailable' and 'path does not exist' in f['message'] for f in missing['findings'])
assert not (pathlib.Path(os.environ['sc_scratch']) / 'does-not-exist').exists()
assert 'html' in (p / 'report-only.stderr').read_text()
CHECK
```

## Gotchas

Launch explicitly enforces HTML with `[html] enforce = "on"`; auto enforcement changes with the selected gates and is a different scenario. Coverage can be unmeasured or advisory for a static fixture. Record the actual gate state; do not claim full coverage from `verdict: pass`. Keep the saved report copies before another invocation overwrites the project's last report. The mutation above is confined to the copied fixture.
