# Analysis scope

Limit analysis to named source paths or changed files, and reject an unresolvable Git base instead of pretending that nothing changed.

## Sub-features

- `paths`: a newline-separated file list selects the requested source path.
- `diff`: an explicit `HEAD` base includes an uncommitted change.
- `missing-base`: a nonexistent base produces an analyzer error.

## How to get to it (user POV)

Pass `--paths FILE` or `--diff BASE` to `sc analyze PATH`. These options are mutually exclusive. A bare `--diff` resolves its own base; shallow-history behavior needs its own fixture when touched.

## Driving it with sc_run

Preconditions: Launch and Doctor completed; Rust and `cargo-llvm-cov` with `llvm-tools-preview` are installed. This recipe creates a separate Rust fixture with no Git history.

```bash
mkdir "$sc_scratch/rust"
cp testdata/good_crate/Cargo.toml "$sc_scratch/rust/"
cp -R testdata/good_crate/src "$sc_scratch/rust/src"
printf 'target/\n.sc/\n' > "$sc_scratch/rust/.gitignore"
printf 'pack = "rust"\n' > "$sc_scratch/rust.toml"
sc_config="$sc_scratch/rust.toml"
printf 'src/lib.rs\n' > "$sc_scratch/paths.txt"
sc_run paths 0 "$sc_scratch/rust" --format json --paths "$sc_scratch/paths.txt"
git -C "$sc_scratch/rust" init -q
git -C "$sc_scratch/rust" add Cargo.toml src .gitignore
git -C "$sc_scratch/rust" -c user.name=Verifier -c user.email=verify@example.invalid -c commit.gpgsign=false commit -qm 'Seed verification fixture'
printf '\n// verification change\n' >> "$sc_scratch/rust/src/lib.rs"
sc_run diff 0 "$sc_scratch/rust" --format json --diff HEAD
sc_run missing-base 2 "$sc_scratch/rust" --format json --diff refs/heads/verification-missing
python3 - <<'CHECK'
import json, os, pathlib
p = pathlib.Path(os.environ['sc_evidence'])
paths = json.loads((p / 'paths.stdout').read_text())
assert paths['scope']['mode'] == 'paths'
assert paths['scope']['paths'] == ['src/lib.rs']
diff = json.loads((p / 'diff.stdout').read_text())
assert diff['scope']['mode'] == 'diff'
assert diff['scope']['base'] == 'HEAD'
assert diff['scope']['paths'] == ['src/lib.rs']
missing = json.loads((p / 'missing-base.stdout').read_text())
assert missing['verdict'] == 'fail'
assert 'verification-missing' in (p / 'missing-base.stderr').read_text()
CHECK
git -C "$sc_scratch/rust" restore src/lib.rs
```

## Gotchas

The fixture's `.sc` directory is output and is deliberately not committed. The web pack currently does not apply `--paths` or `--diff`; use the Rust fixture for this proof and report web scope as a product gap if requested. Source scope and secrets scope can differ; a selected source path does not prove secrets elsewhere were skipped. Cleanup removes the entire disposable Git repository; it never changes the checkout's Git history or identity.
