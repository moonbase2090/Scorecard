#!/usr/bin/env python3
"""Check the docs against the real CLI.

1. Runs every fenced block whose info string starts with ``bash doctest`` in
   README.md and docs/**/*.md, each in a fresh git repo copied from testdata.
2. Checks that the reference names every `sc analyze` flag, config key, gate,
   and rule id the code defines, and that every `sc` command and flag the docs
   show in code exists in `sc --help`.
3. Checks that release links and version pins use the Cargo.toml version.
4. Checks that relative Markdown links resolve.

Usage:
  scripts/check-docs.py --sc target/release/sc [--network]

Block options, after ``bash doctest`` in the info string:
  exit=N         expected exit status of the whole block (default 0)
  project=NAME   testdata directory to copy as the working tree (default good_crate)
  network        needs the network; runs only with --network
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FENCE = re.compile(r"^```bash doctest([^\n]*)\n(.*?)^```\s*$", re.M | re.S)
RULE_PREFIXES = (
    "compile|complexity|config|coverage|crap|engine|html|links|lint|mutation|perf|sca|secrets|spec|test"
)
RULE_LITERAL = re.compile(rf'"((?:{RULE_PREFIXES})\.[a-z_]+)"')
NOT_RULES = {"mutation.diff", "config.env"}
FILE_SUFFIXES = {
    "rs", "toml", "json", "md", "html", "htm", "txt", "py", "js", "xml", "out", "info",
    "sh", "diff", "lock", "yml", "yaml", "sarif", "go", "cs", "php", "java", "cpp", "lcov",
}
SECTIONS = {
    "GatesConfig": "gates",
    "ScopeConfig": "scope",
    "MutationConfig": "mutation",
    "LlmConfig": "llm",
    "EnginesConfig": "engines",
    "CommandsConfig": "commands",
    "HtmlConfig": "html",
    "LinksConfig": "links",
    "A11yConfig": "a11y",
}


def doc_files() -> list[Path]:
    return [ROOT / "README.md", *sorted((ROOT / "docs").rglob("*.md"))]


def run_blocks(sc: Path, network: bool) -> list[str]:
    errors = []
    sc_dir = str(sc.resolve().parent)
    real_home = os.environ.get("HOME", "")
    for path in doc_files():
        rel = path.relative_to(ROOT)
        text = path.read_text()
        for match in FENCE.finditer(text):
            options = dict(
                opt.split("=", 1) if "=" in opt else (opt, "1")
                for opt in match.group(1).split()
            )
            line = text[: match.start()].count("\n") + 1
            label = f"{rel}:{line}"
            if "network" in options and not network:
                print(f"skip {label} (network)")
                continue
            project = options.get("project", "good_crate")
            expected = int(options.get("exit", "0"))
            with tempfile.TemporaryDirectory(prefix="sc-docs-") as tmp:
                work = Path(tmp) / "project"
                shutil.copytree(ROOT / "testdata" / project, work)
                home = Path(tmp) / "home"
                home.mkdir()
                git = ["git", "-c", "user.name=docs", "-c", "user.email=docs@example.invalid"]
                subprocess.run(["git", "init", "-q"], cwd=work, check=True)
                subprocess.run(["git", "add", "-A"], cwd=work, check=True)
                subprocess.run([*git, "commit", "-qm", "init"], cwd=work, check=True)
                env = dict(os.environ)
                # Keep the toolchains; isolate everything else sc may read.
                env.setdefault("RUSTUP_HOME", os.path.join(real_home, ".rustup"))
                env.setdefault("CARGO_HOME", os.path.join(real_home, ".cargo"))
                env.update(
                    HOME=str(home),
                    PATH=f"{sc_dir}{os.pathsep}{env.get('PATH', '')}",
                    SCORECARD_DOCS_BINARY=str(sc.resolve()),
                    NO_COLOR="1",
                    GIT_AUTHOR_NAME="docs",
                    GIT_AUTHOR_EMAIL="docs@example.invalid",
                    GIT_COMMITTER_NAME="docs",
                    GIT_COMMITTER_EMAIL="docs@example.invalid",
                )
                for key in ("XAI_API_KEY", "OPENROUTER_API_KEY"):
                    env.pop(key, None)
                script = "set -euo pipefail\n" + match.group(2)
                try:
                    done = subprocess.run(
                        ["bash", "-c", script],
                        cwd=work,
                        env=env,
                        capture_output=True,
                        text=True,
                        timeout=600,
                    )
                    status = done.returncode
                    output = done.stdout[-1500:] + done.stderr[-1500:]
                except subprocess.TimeoutExpired:
                    status, output = -1, "timed out after 600s"
            if status == expected:
                print(f"ok   {label} (exit {status})")
            else:
                errors.append(f"{label}: exit {status}, expected {expected}\n{output}")
    return errors


def sc_options(sc: Path) -> dict[str, set[str]]:
    """Options of `sc` (key "") and of each subcommand, from their --help.

    Nested commands are keyed by the full path, such as `config init`.
    """

    def help_text(*args: str) -> str:
        return subprocess.run([str(sc), *args, "--help"], capture_output=True, text=True, check=True).stdout

    def options(text: str) -> set[str]:
        return set(re.findall(r"(?<![\w-])(--[a-z][a-z-]+|-[a-zA-Z])\b", text))

    def commands(text: str) -> list[str]:
        if "Commands:" not in text:
            return []
        listed = text.split("Commands:", 1)[1].split("Options:", 1)[0]
        return re.findall(r"^  ([a-z][a-z-]*) ", listed, re.M)

    result: dict[str, set[str]] = {}

    def walk(prefix: list[str]) -> None:
        text = help_text(*prefix)
        key = " ".join(prefix)
        # clap's own `help` subcommand has no --help of its own.
        result[key] = set() if prefix[-1:] == ["help"] else options(text)
        for command in commands(text):
            if command == "help":
                result[" ".join(prefix + [command])] = set()
            else:
                walk(prefix + [command])

    top = help_text()
    result[""] = options(top)
    for command in commands(top):
        if command == "help":
            result[command] = set()
        else:
            walk([command])
    return result


# `sc` in command position: line start (after `$`, `exec`, or `sudo`) or after | ; & (
INVOCATION = re.compile(
    r"(?:^\s*(?:\$\s+)?(?:(?:exec|sudo)\s+)?|[|;&(]\s*)(?:\./)?sc(?=\s|$)((?:[ \t]+[^\s|;&>#`'\"]+)*)"
)


def code_lines(text: str):
    """(line number, text) for fenced-block lines and inline code spans."""
    fenced = False
    for number, line in enumerate(text.splitlines(), 1):
        if line.startswith("```"):
            fenced = not fenced
        elif fenced:
            yield number, line
        else:
            for span in re.findall(r"`([^`]+)`", line):
                yield number, span


def check_invocations(options: dict[str, set[str]]) -> list[str]:
    errors = []
    for path in doc_files():
        rel = path.relative_to(ROOT)
        for number, line in code_lines(path.read_text()):
            for match in INVOCATION.finditer(line):
                words = match.group(1).split()
                if not words or words[0][0].isdigit():
                    continue  # bare `sc`, or a version line such as `sc 0.1.4`
                parts: list[str] = []
                index = 0
                if not words[0].startswith("-"):
                    while index < len(words) and not words[index].startswith("-"):
                        candidate = " ".join(parts + [words[index]])
                        if candidate not in options:
                            break
                        parts.append(words[index])
                        index += 1
                command = " ".join(parts)
                label = f"sc {command}".strip()
                where = f"{rel}:{number}: `sc{match.group(1)}`"
                if command not in options:
                    errors.append(f"{where}: `{label}` is not an sc command. Check `sc --help` and update this line.")
                    continue
                for word in words[index:]:
                    flag = word.split("=", 1)[0]
                    if flag.startswith("-") and flag not in options[command] | {"--help", "-h"}:
                        errors.append(
                            f"{where}: `{flag}` is not an option of `{label}`."
                            f" Check `{label} --help` and update this line."
                        )
    return errors


def config_keys() -> set[str]:
    text = (ROOT / "crates/sc-core/src/config.rs").read_text()
    keys = {"pack", "toolchain"}
    for name, section in SECTIONS.items():
        body = re.search(rf"pub struct {name} \{{(.*?)^\}}", text, re.S | re.M)
        if not body:
            raise SystemExit(f"config.rs: struct {name} not found")
        for field in re.findall(r"pub (\w+):", body.group(1)):
            keys.add(f"{section}.{field}")
    return keys


def gates() -> set[str]:
    text = (ROOT / "crates/sc-core/src/config.rs").read_text()
    body = re.search(r"KNOWN_GATES: &\[&str\] = &\[(.*?)\];", text, re.S)
    return set(re.findall(r'"([a-z0-9]+)"', body.group(1)))


def rule_ids() -> set[str]:
    found = set()
    for path in (ROOT / "crates").rglob("*.rs"):
        if "/tests/" in str(path):
            continue
        for rule in RULE_LITERAL.findall(path.read_text()):
            if rule.rsplit(".", 1)[1] not in FILE_SUFFIXES and rule not in NOT_RULES:
                found.add(rule)
    return found


def check_reference(sc: Path) -> list[str]:
    errors = []
    ref = ROOT / "docs/reference"
    cli = (ref / "cli.md").read_text()
    for flag in sorted(sc_options(sc)["analyze"] - {"--help", "-h"}):
        if f"`{flag}" not in cli:
            errors.append(f"docs/reference/cli.md: missing flag {flag}")
    config = (ref / "config.md").read_text()
    for key in sorted(config_keys()):
        if f"`{key}`" not in config:
            errors.append(f"docs/reference/config.md: missing key {key}")
    gate_doc = (ref / "gates.md").read_text()
    for gate in sorted(gates()):
        if f"`{gate}`" not in gate_doc:
            errors.append(f"docs/reference/gates.md: missing gate {gate}")
    rules = (ref / "rules.md").read_text()
    for rule in sorted(rule_ids()):
        if f"`{rule}`" not in rules:
            errors.append(f"docs/reference/rules.md: missing rule {rule}")
    return errors


def check_versions() -> list[str]:
    manifest = (ROOT / "Cargo.toml").read_text()
    version = re.search(r'(?ms)^\[workspace\.package\].*?^version = "([^"]+)"', manifest).group(1)
    errors = []
    patterns = [
        r"releases/download/v(\d+\.\d+\.\d+)",
        r"releases/tag/v(\d+\.\d+\.\d+)",
        r"\bsc-v(\d+\.\d+\.\d+)-",
        r"\bVERSION=v(\d+\.\d+\.\d+)",
        r"Scorecard/action@v(\d+\.\d+\.\d+)",
    ]
    for path in doc_files():
        text = path.read_text()
        for pattern in patterns:
            for found in re.findall(pattern, text):
                if found != version:
                    errors.append(f"{path.relative_to(ROOT)}: v{found} should be v{version}")
    return errors


def check_links() -> list[str]:
    errors = []
    link = re.compile(r"\]\(([^)\s#]+)(?:#[^)]*)?\)")
    for path in doc_files():
        for target in link.findall(path.read_text()):
            if re.match(r"[a-z]+:", target):
                continue
            if not (path.parent / target).exists():
                errors.append(f"{path.relative_to(ROOT)}: broken link {target}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--sc", required=True, type=Path)
    parser.add_argument("--network", action="store_true")
    args = parser.parse_args()
    errors = check_versions() + check_links() + check_reference(args.sc)
    errors += check_invocations(sc_options(args.sc))
    errors += run_blocks(args.sc, args.network)
    if errors:
        print("\n".join(errors), file=sys.stderr)
        print(f"\n{len(errors)} docs problem(s)", file=sys.stderr)
        return 1
    print("docs match the CLI")
    return 0


if __name__ == "__main__":
    sys.exit(main())
