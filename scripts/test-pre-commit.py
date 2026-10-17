#!/usr/bin/env python3
"""Exercise the hook against real Git indexes and cargo fmt."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent


def run(*args, check=True):
    return subprocess.run(args, check=check, capture_output=True, text=True)


with tempfile.TemporaryDirectory() as directory:
    os.chdir(directory)
    run("git", "init", "-q")
    Path(".githooks").mkdir()
    shutil.copy2(ROOT / ".githooks/pre-commit", ".githooks/pre-commit")
    run("sh", str(ROOT / "scripts/install-hooks.sh"))
    assert run("git", "config", "core.hooksPath").stdout.strip() == ".githooks"
    run("sh", str(ROOT / "scripts/install-hooks.sh"))
    Path("Cargo.toml").write_text(
        '[package]\nname = "fixture"\nversion = "0.1.0"\nedition = "2021"\n'
    )
    run("git", "add", "Cargo.toml")
    assert run("sh", ".githooks/pre-commit", check=False).returncode == 0
    print("PASS: installer is repeatable; no Rust changes skip")

    source = Path("staged space\nfile.rs")
    clean = "fn main() {\n    println!(\"hello\");\n}\n"
    dirty = 'fn main(){println!("hello");}\n'
    source.write_text(dirty)
    run("git", "add", str(source))
    source.write_text(clean)
    result = run("sh", ".githooks/pre-commit", check=False)
    assert result.returncode == 1, result.stdout + result.stderr
    assert "Fix: cargo fmt --all" in result.stderr
    assert source.read_text() == clean
    assert run("git", "show", ":" + str(source)).stdout == dirty
    print("PASS: malformed index fails despite clean working file; neither is changed")

    run("git", "add", str(source))
    source.write_text(dirty)
    result = run("sh", ".githooks/pre-commit", check=False)
    assert result.returncode == 0, result.stdout + result.stderr
    assert source.read_text() == dirty
    print("PASS: clean index passes despite malformed working file; unusual names work")

    run("git", "rm", "--cached", "-f", str(source))
    Path("parent.rs").write_text("mod child;\n")
    Path("child.rs").write_text(dirty)
    run("git", "add", "parent.rs")
    result = run("sh", ".githooks/pre-commit", check=False)
    assert result.returncode == 0, result.stdout + result.stderr
    print("PASS: unstaged child modules are not checked")
