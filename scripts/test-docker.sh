#!/usr/bin/env bash
# The runtime image stays a small Debian stage, and the workflow pushes only on release tags.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
python3 - "$root" <<'PY'
import re
import sys
from pathlib import Path

root = Path(sys.argv[1])
dockerfile = (root / "Dockerfile").read_text()
froms = [line for line in dockerfile.splitlines() if line.startswith("FROM ")]
if len(froms) < 2:
    raise SystemExit("Dockerfile must be multi-stage")
if "rust:" not in froms[0]:
    raise SystemExit(f"builder stage should be the Rust image, got {froms[0]}")
if not froms[-1].startswith("FROM debian:bookworm-slim"):
    raise SystemExit(f"runtime stage should be debian:bookworm-slim, got {froms[-1]}")
if "rust:" in froms[-1]:
    raise SystemExit("runtime stage uses the Rust toolchain image")
for required in (
    "cargo build --release --locked -p sc-cli -p sc-mcp",
    "COPY --from=build",
    "/usr/local/bin/",
    "ca-certificates git",
):
    if required not in dockerfile:
        raise SystemExit(f"Dockerfile is missing {required}")

ignore = (root / ".dockerignore").read_text().splitlines()
if "target" not in ignore:
    raise SystemExit(".dockerignore must exclude target")

readme = (root / "README.md").read_text()
for required in (
    "https://raw.githubusercontent.com/moonbase2090/Scorecard/main/install.sh",
    "ghcr.io/moonbase2090/scorecard",
):
    if required not in readme:
        raise SystemExit(f"README is missing {required}")

workflow = (root / ".github/workflows/docker.yml").read_text()
if "pull_request:" not in workflow or 'tags:' not in workflow or '- "v*"' not in workflow:
    raise SystemExit("docker workflow must build on pull requests and on v* tags")
secrets = set(re.findall(r"secrets\.([A-Za-z0-9_]+)", workflow))
if not secrets <= {"GITHUB_TOKEN"}:
    raise SystemExit(f"docker workflow uses unexpected secrets: {sorted(secrets)}")
for banned in ("APPLE_", "PRIVATE_KEY", "RELEASE_DENYLIST"):
    if banned in workflow:
        raise SystemExit(f"docker workflow references {banned}")

step_if = None
pushes = 0
for line in workflow.splitlines():
    stripped = line.strip()
    if stripped.startswith("- name:") or stripped.startswith("- uses:"):
        step_if = None
    if stripped.startswith("if:"):
        step_if = stripped
    if "docker push " in line or stripped == "push: true":
        pushes += 1
        if step_if is None or "refs/tags/v" not in step_if:
            raise SystemExit(f"push is not limited to release tags: {stripped}")
if pushes < 1:
    raise SystemExit("docker workflow does not push a release image")
print(f"docker contract ok ({pushes} guarded pushes)")
PY
