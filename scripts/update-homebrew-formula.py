#!/usr/bin/env python3
"""Generate the Homebrew formula for a tagged Scorecard release."""

from __future__ import annotations

import re
import sys
from pathlib import Path


PLATFORMS = {
    "on_macos": {
        "on_arm": "aarch64-apple-darwin",
        "on_intel": "x86_64-apple-darwin",
    },
    "on_linux": {
        "on_arm": "aarch64-unknown-linux-gnu",
        "on_intel": "x86_64-unknown-linux-gnu",
    },
}


def load_checksums(path: Path) -> dict[str, str]:
    checksums: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  (\S+)", line)
        if match:
            checksums[match.group(2)] = match.group(1)
    return checksums


def formula(version: str, checksums: dict[str, str]) -> str:
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?", version):
        raise ValueError(f"invalid release version: {version}")

    sections: list[str] = []
    for platform, architectures in PLATFORMS.items():
        architecture_sections = []
        for architecture, target in architectures.items():
            filename = f"sc-v{version}-{target}.tar.gz"
            sha = checksums.get(filename)
            if sha is None:
                raise ValueError(f"missing checksum for {filename}")
            architecture_sections.append(
                f"    {architecture} do\n"
                f'      url "https://github.com/moonbase2090/Scorecard/releases/download/v{version}/{filename}"\n'
                f'      sha256 "{sha}"\n'
                "    end"
            )
        sections.append(f"  {platform} do\n" + "\n".join(architecture_sections) + "\n  end")

    return (
        "class Sc < Formula\n"
        "  desc \"Code quality and test coverage analysis for software repositories\"\n"
        "  homepage \"https://github.com/moonbase2090/Scorecard\"\n"
        '  license any_of: ["Apache-2.0", "MIT"]\n\n'
        + "\n\n".join(sections)
        + "\n\n"
        "  def install\n"
        '    bin.install "sc", "sc-mcp"\n'
        "  end\n\n"
        "  test do\n"
        '    assert_match version.to_s, shell_output("#{bin}/sc --version")\n'
        '    assert_match version.to_s, shell_output("#{bin}/sc-mcp --version")\n'
        "  end\n"
        "end\n"
    )


def main() -> int:
    if len(sys.argv) != 4:
        print("usage: update-homebrew-formula.py VERSION SHA256SUMS OUTPUT", file=sys.stderr)
        return 2

    version, checksums_path, output_path = sys.argv[1:]
    if version.startswith("v"):
        version = version[1:]

    try:
        content = formula(version, load_checksums(Path(checksums_path)))
    except (OSError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1

    output = Path(output_path)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(content, encoding="utf-8")
    print(f"updated {output} for v{version}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
