#!/usr/bin/env python3
"""Exercise the installed hook with real commits, Rust formatting, and Biome."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--hook', type=Path, default=ROOT / '.githooks/pre-commit')
hook = parser.parse_args().hook.resolve()

with tempfile.TemporaryDirectory(prefix='format-hook-') as directory:
    repo = Path(directory)

    def run(*command):
        result = subprocess.run(command, cwd=repo, capture_output=True, text=True)
        assert result.returncode == 0, result.stdout + result.stderr
        return result.stdout

    def commit():
        return subprocess.run(
            ['git', '-c', 'commit.gpgsign=false', 'commit', '-qm', 'Check formatting'],
            cwd=repo, capture_output=True, text=True,
        )

    def accepted():
        result = commit()
        assert result.returncode == 0, result.stdout + result.stderr

    run('git', 'init', '-q')
    run('git', 'config', 'user.name', 'Formatting test')
    run('git', 'config', 'user.email', 'formatting@example.invalid')
    (repo / '.githooks').mkdir()
    shutil.copy2(hook, repo / '.githooks/pre-commit')
    os.chmod(repo / '.githooks/pre-commit', 0o755)
    run('sh', str(ROOT / 'scripts/install-hooks.sh'))
    run('sh', str(ROOT / 'scripts/install-hooks.sh'))
    assert run('git', 'config', 'core.hooksPath').strip() == '.githooks'
    (repo / 'Cargo.toml').write_text(
        '[package]\nname = "format-check"\nversion = "0.1.0"\nedition = "2021"\n'
    )
    run('git', 'add', 'Cargo.toml')
    accepted()
    assert run('git', 'show', 'HEAD:Cargo.toml').startswith('[package]\n')
    print('PASS: installer is repeatable; commit without Rust or web changes succeeds')

    for filename, dirty, clean in [
        ('staged space\nfile.rs', 'fn main(){println!("hello");}\n',
         'fn main() {\n    println!("hello");\n}\n'),
        ('web space\nfile.js', 'const answer=42\n', 'const answer = 42;\n'),
    ]:
        source = repo / filename
        old_head = run('git', 'rev-parse', 'HEAD')
        source.write_text(dirty)
        run('git', 'add', '--', filename)
        source.write_text(clean)
        rejected = commit()
        assert rejected.returncode != 0, f'Committed malformed index for {filename}'
        assert 'format' in rejected.stderr.lower(), rejected.stderr
        assert run('git', 'rev-parse', 'HEAD') == old_head
        assert run('git', 'show', ':' + filename) == dirty
        assert source.read_text() == clean
        print(f'PASS: {source.suffix} malformed index rejected despite clean worktree; both unchanged')

        run('git', 'add', '--', filename)
        source.write_text(dirty)
        accepted()
        assert run('git', 'show', 'HEAD:' + filename) == clean
        assert run('git', 'show', ':' + filename) == clean
        assert source.read_text() == dirty
        source.write_text(clean)
        print(f'PASS: {source.suffix} clean index committed despite malformed worktree; unusual name preserved')

    (repo / 'parent.rs').write_text('mod child;\n')
    (repo / 'child.rs').write_text('fn child(){println!("unstaged");}\n')
    run('git', 'add', 'parent.rs')
    accepted()
    assert run('git', 'show', 'HEAD:parent.rs') == 'mod child;\n'
    assert 'child.rs' not in run('git', 'ls-tree', '--name-only', 'HEAD').splitlines()
    assert (repo / 'child.rs').read_text() == 'fn child(){println!("unstaged");}\n'
    print('PASS: unstaged child module stays unchecked; staged Rust outside Cargo targets commits')

    staged_config = '{\n    "formatter": {\n        "indentStyle": "space",\n        "indentWidth": 4\n    }\n}\n'
    working_config = staged_config.replace('"indentWidth": 4', '"indentWidth": 2')
    configured_js = 'function answer() {\n    return 42;\n}\n'
    (repo / 'biome.json').write_text(staged_config)
    (repo / 'configured.js').write_text(configured_js)
    run('git', 'add', 'biome.json', 'configured.js')
    (repo / 'biome.json').write_text(working_config)
    accepted()
    assert run('git', 'show', 'HEAD:biome.json') == staged_config
    assert run('git', 'show', ':biome.json') == staged_config
    assert (repo / 'biome.json').read_text() == working_config
    assert run('git', 'show', 'HEAD:configured.js') == configured_js
    print('PASS: web formatting uses staged configuration and preserves dirty working configuration')

    run('git', 'mv', 'configured.js', 'renamed file.js')
    (repo / 'renamed file.js').write_text('function answer(){return 42}\n')
    accepted()
    assert run('git', 'show', 'HEAD:renamed file.js') == configured_js
    assert (repo / 'renamed file.js').read_text() == 'function answer(){return 42}\n'
    print('PASS: renamed web file commits staged formatting and preserves dirty worktree')

    run('git', 'rm', '-f', '--', 'renamed file.js', 'web space\nfile.js')
    accepted()
    assert run('git', 'ls-tree', '--name-only', 'HEAD', '--', 'renamed file.js') == ''
    assert run('git', 'ls-tree', '--name-only', 'HEAD', '--', 'web space\nfile.js') == ''
    print('PASS: staged web deletions commit without checking deleted files')

    (repo / 'notes.txt').write_text('Only documentation changes.\n')
    run('git', 'add', 'notes.txt')
    rust_file = repo / 'staged space\nfile.rs'
    rust_file.write_text('fn main(){println!("unstaged");}\n')
    accepted()
    assert run('git', 'show', 'HEAD:notes.txt') == 'Only documentation changes.\n'
    assert run('git', 'show', 'HEAD:staged space\nfile.rs') == 'fn main() {\n    println!("hello");\n}\n'
    assert rust_file.read_text() == 'fn main(){println!("unstaged");}\n'
    assert (repo / 'biome.json').read_text() == working_config
    print('PASS: unrelated commit skips unstaged Rust and formatter configuration changes')
