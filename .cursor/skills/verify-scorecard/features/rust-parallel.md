# Parallel Rust engines

Preconditions: Launch and Doctor completed. This drive creates a disposable Rust crate with no external dependencies and runs the real CLI with parallel engine scheduling enabled.

```bash
sc_parallel="$sc_scratch/rust-parallel"
mkdir -p "$sc_parallel/src"
cat > "$sc_parallel/Cargo.toml" <<'EOF'
[package]
name = "parallel_fixture"
version = "0.1.0"
EOF
sc_fake_key="AKIA$(printf '0Z3VS5J4')$(printf 'AB3KQM9Z')"
printf 'use undeclared_crate::Thing;\nconst KEY: &str = "%s";\nfn main() {}\n' "$sc_fake_key" > "$sc_parallel/src/main.rs"
cat > "$sc_parallel/analyzer.toml" <<'EOF'
pack = "rust"
[engines]
parallel = true
coverage = false
[commands]
lint = ""
[gates]
fail_on = ["none"]
EOF
sc_config="$sc_parallel/analyzer.toml"
export sc_config
sc_run parallel-rust 0 "$sc_parallel"
python3 - <<'CHECK'
import json, os, pathlib
evidence = pathlib.Path(os.environ['sc_evidence'])
report = json.loads((pathlib.Path(os.environ['sc_scratch']) / 'rust-parallel/.sc/last-scorecard.json').read_text())
rules = {finding['rule'] for finding in report['findings']}
assert 'sca.undeclared_dependency' in rules, rules
assert 'secrets.aws_access_key' in rules, rules
assert 'sca' in report['engines_run'], report['engines_run']
assert 'secrets' in report['engines_run'], report['engines_run']
CHECK
```

The unresolved import makes compilation fail, but `fail_on = ["none"]` keeps the CLI drive focused on the reports from both independent scans. The key is a detector fixture assembled in the disposable tree, not a credential. The source should produce one SCA finding and one AWS-key finding in the persisted report.
