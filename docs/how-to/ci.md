# Run sc in CI

## GitHub Actions

```yaml
name: scorecard
on: [pull_request]
permissions:
  contents: read
  security-events: write   # SARIF upload to code scanning
jobs:
  sc:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0     # --diff needs the base commit
      - uses: moonbase2090/Scorecard/action@v0.1.6
        with:
          diff: origin/${{ github.base_ref }}
```

The action installs `sc`, runs `sc analyze .`, and fails the job when a gate selected by `fail-on` fails. The report verdict remains `FAIL` for any enforced gate failure, even if `fail-on` lets the job exit 0. In that case, `sc` writes one warning to stderr with the failed gate names. Set `fail-on: none` to keep the report and return success for every gate. An empty value is invalid. Omit the input to use the default gates. On public repositories the SARIF report appears in the pull request's code scanning alerts. Only `secrets.*` findings upload at error level; every other rule (test failures, missing coverage, lint, CRAP) uploads at warning, so quality signals never count as security vulnerabilities. With `format: html` or `all`, the report is uploaded as the `scorecard-report` artifact.

| Input | Default | Meaning |
|---|---|---|
| `fail-on` | `types,tests,crap,secrets,lint` | Comma-separated gates whose failures fail the job. Use `none` by itself to suppress gate failures from the exit code. Empty is invalid. |
| `diff` | `""` | Base ref for `--diff`. Empty scores the whole tree. On a pull request, pass the base ref so the report covers changed code and a one-line count of the rest of the tree. |
| `format` | `sarif` | `json`, `md`, `sarif`, `html`, or `all` |
| `spec` | `""` | Path for `--spec` |
| `mutation` | `off` | `off`, `diff`, or `full` |
| `version` | the action's tag | `sc` release to install, such as `0.1.6` |
| `budget-seconds` | `120` | Wall-clock budget for pack commands (`--budget-seconds` on `sc analyze`) |

The runner needs the tools your pack uses (a Rust toolchain, Python with pytest, and so on); see [packs](../packs.md). A missing coverage tool does not fail the job, and the report says coverage was not measured.

Do not extract a release tarball inside your repository checkout. The archive contains `sc` and `sc-mcp` binaries; unpacking them next to your source makes `sc analyze` scan those files and can raise false `secrets.*` findings. Use `$RUNNER_TEMP` (or another directory outside the tree) for download and extract, then `install` the binaries onto `PATH`. Allocate a new `mktemp -d` subdirectory for each install so a leftover flat `sc` binary cannot shadow a newer nested bundle (`scripts/install-release-tarball.sh` wraps the same steps).

## Any other CI

Download and install outside the checkout (example uses `$RUNNER_TEMP` on GitHub Actions):

```bash doctest network
VER=0.1.6
case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) TRIPLE=aarch64-apple-darwin ;;
  Darwin-x86_64) TRIPLE=x86_64-apple-darwin ;;
  Linux-aarch64) TRIPLE=aarch64-unknown-linux-gnu ;;
  Linux-x86_64) TRIPLE=x86_64-unknown-linux-gnu ;;
  *) TRIPLE=x86_64-unknown-linux-gnu ;;
esac
STAGE="$(mktemp -d "${RUNNER_TEMP:-/tmp}/sc-install.XXXXXX")"
curl -fsSL -o "$STAGE/scorecard.tar.gz" \
  "https://github.com/moonbase2090/Scorecard/releases/download/v$VER/sc-v${VER}-${TRIPLE}.tar.gz"
tar -xzf "$STAGE/scorecard.tar.gz" -C "$STAGE"
root="$STAGE"
if [ -f "$STAGE/sc" ]; then
  : # flat layout (e.g. v0.1.6)
elif comp="$(find "$STAGE" -maxdepth 1 -type d -name 'sc-v*' | head -1)" && [ -n "$comp" ]; then
  root="$comp"
else
  echo "release archive has no sc binary" >&2
  exit 1
fi
mkdir -p "$HOME/.local/bin"
install -m 755 "$root/sc" "$root/sc-mcp" "$HOME/.local/bin/"
export PATH="$HOME/.local/bin:$PATH"
sc analyze . --format all --out sc-report --budget-seconds 600 >/dev/null
```

The exit status is the gate result (0 pass, 1 fail, 2 could not run). Keep `sc-report.html` as a build artifact and upload `sc-report.sarif` to any SARIF viewer. `--budget-seconds` raises the 120-second wall-clock budget for pack commands when the suite is slow.
