#!/usr/bin/env python3
"""Check the installed hook using real Git commits, cargo fmt, and Biome."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--hook', type=Path, default=ROOT / '.githooks/pre-commit')
args = parser.parse_args()
hook = args.hook.resolve()

with tempfile.TemporaryDirectory(prefix='format-hook-') as directory:
    repo = Path(directory)

    def run(*command, expected=0):
        result = subprocess.run(command, cwd=repo, capture_output=True, text=True)
        assert result.returncode == expected, (
            f'{command}: expected exit {expected}, got {result.returncode}\n'
            + result.stdout + result.stderr
        )
        return result.stdout

    def commit():
        return subprocess.run(
            ['git', '-c', 'commit.gpgsign=false', 'commit', '-qm', 'Check formatting'],
            cwd=repo, capture_output=True, text=True,
        )

    run('git', 'init', '-q')
    run('git', 'config', 'user.name', 'Formatting test')
    run('git', 'config', 'user.email', 'formatting@example.invalid')
    (repo / '.githooks').mkdir()
    shutil.copy2(hook, repo / '.githooks/pre-commit')
    os.chmod(repo / '.githooks/pre-commit', 0o755)
    run('sh', str(ROOT / 'scripts/setup-hooks.sh'))
    run('sh', str(ROOT / 'scripts/setup-hooks.sh'))
    assert run('git', 'config', 'core.hooksPath').strip() == '.githooks'
    (repo / 'Cargo.toml').write_text(
        '[package]\nname = "format-check"\nversion = "0.1.0"\nedition = "2021"\n'
    )
    (repo / 'src').mkdir()
    (repo / 'src/main.rs').write_text('fn main() {}\n')
    run('git', 'add', 'Cargo.toml', 'src/main.rs')
    initial = commit()
    assert initial.returncode == 0, initial.stdout + initial.stderr

    for filename, dirty, clean in [
        ('src/main.rs', 'fn main(){println!("hello");}\n',
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
        assert 'stage the formatted files with git add' in rejected.stderr
        assert run('git', 'rev-parse', 'HEAD') == old_head
        assert run('git', 'show', ':' + filename) == dirty
        assert source.read_text() == clean
        print(f'PASS: {source.suffix} malformed index rejected despite formatted worktree; both unchanged')

        run('git', 'add', '--', filename)
        source.write_text(dirty)
        accepted = commit()
        assert accepted.returncode == 0, accepted.stdout + accepted.stderr
        assert run('git', 'show', 'HEAD:' + filename) == clean
        assert run('git', 'show', ':' + filename) == clean
        assert source.read_text() == dirty
        source.write_text(clean)
        print(f'PASS: {source.suffix} formatted index committed despite malformed worktree; worktree preserved')

    run('git', 'rm', '--', 'web space\nfile.js')
    deleted = commit()
    assert deleted.returncode == 0, deleted.stdout + deleted.stderr
    assert run('git', 'ls-tree', '--name-only', 'HEAD', '--', 'web space\nfile.js') == ''
    print('PASS: staged deletion commits without checking deleted file')

    (repo / 'notes.txt').write_text('Only documentation changes.\n')
    run('git', 'add', 'notes.txt')
    (repo / 'src/main.rs').write_text('fn main(){println!("unstaged");}\n')
    notes = commit()
    assert notes.returncode == 0, notes.stdout + notes.stderr
    assert run('git', 'show', 'HEAD:notes.txt') == 'Only documentation changes.\n'
    assert run('git', 'show', 'HEAD:src/main.rs') == 'fn main() {\n    println!("hello");\n}\n'
    print('PASS: unrelated commit skips unstaged formatting changes')
