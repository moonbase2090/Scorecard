# Releasing Scorecard

Releases are cut from `main` by pushing a `vX.Y.Z` tag; see `.github/workflows/release.yml`.

## Public release text

Release text and artifacts must not reference private infrastructure, hostnames, personal paths, or internal tooling. That includes release titles and bodies, tag messages, `CHANGELOG.md`, README and docs, and strings embedded in shipped binaries and archives.

Before publish, CI runs `scripts/check-release-text.sh` on those sources plus built assets in `dist/`. The checker unpacks `.tar.gz`, `.pkg` (`bsdtar` / `xar` / `pkgutil`), `.dmg` (`7z` on Linux, `hdiutil` on macOS), and `.zip`, including nested archives inside those trees; unpack or scan errors fail the run. Linux publish installs `libarchive-tools` and `p7zip-full` for `.pkg` and `.dmg`. Optional deny rules may live in `~/.config/moonbase/release-denylist.txt` (local, untracked) or the `RELEASE_DENYLIST` secret in CI (newline-separated extended regexes). Invalid configured patterns or incomplete scans exit non-zero (fail closed). Missing optional deny lists are ignored. An unmodified Mozilla Public License 2.0 file is checked with the built-in rules only; optional rules still apply when that file differs. Release archives record a neutral owner, and release builds remap absolute build paths out of the binaries.

## Maintainer checklist

Registry publication is disabled. After the release assets are published, verify the binary download metadata with `python3 scripts/check-binstall.py`; it runs `cargo-binstall --dry-run` against both binary manifests and every release target without installing binaries. See [cargo-binstall checks and registry prerequisites](docs/how-to/cargo-binstall.md). Do not treat a passing binary check as a successful `cargo publish --dry-run`.

1. Move `[Unreleased]` entries in `CHANGELOG.md` into the version section for the tag.
2. Confirm tag message and GitHub release title/body contain no private details.
3. Push the tag; wait for the release workflow (signing must succeed before publish).
