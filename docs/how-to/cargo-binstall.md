# Check binary installation with cargo-binstall

The binary manifests describe the existing GitHub release archives for `cargo-binstall`. Registry publication is disabled. Use a local manifest from a clone of this repository; `cargo binstall sc-cli` without the local manifest selects an unrelated crates.io package.

Install the verification tool:

```bash
cargo install cargo-binstall --locked
```

Fetch and check the release archive for your platform without installing Scorecard:

```bash
cargo binstall --manifest-path crates/sc-cli/Cargo.toml --dry-run --no-confirm sc-cli
cargo binstall --manifest-path crates/sc-mcp/Cargo.toml --dry-run --no-confirm sc-mcp
```

To install the fetched binaries, remove `--dry-run`. The local manifests must match a published release version. Unsupported targets fail rather than falling back to another crate or compiling from crates.io.

Verify both manifests against every release target:

```bash
python3 scripts/check-binstall.py
```

The checker downloads and extracts the archives in temporary directories, verifies the declared binary paths, and checks that no binary was installed. It uses only the repository's release metadata, disables GitHub token discovery, and tests arm64 and x86_64 macOS, arm64 and x86_64 GNU/Linux, and x86_64 Windows. The Windows archive is a zip; Unix archives are gzip-compressed tar files. Both use `sc-vVERSION-TARGET/` as the top-level directory. A missing release or target asset fails the check.

Pass `--binstall /path/to/cargo-binstall` if the tool is outside `PATH`.

## Registry publication prerequisites

`cargo publish --dry-run --locked -p sc-cli` currently fails because `publish = false`. This guard remains intentional. No registry upload is part of the metadata verification.

Before enabling registry publication:

1. Choose and recheck the crate name. `sc-cli` belongs to another project, so this repository does not publish under that name. Install from the [repository README](../../README.md). The registry API returned 404 for `scorecard-cli` on 2026-11-02; availability must be rechecked and the name is not reserved.
2. Give internal path dependencies registry versions and publish their dependency graph in order. The workspace currently marks every package `publish = false`.
3. Make the Git-only TUI dependencies resolvable from the registry, including optional dependencies. The pinned TUI package names returned 404 in registry checks on 2026-11-02; adding version strings alone cannot resolve unpublished packages.
4. Bundle the embedded skill inside the CLI crate. `src/skills.rs` currently includes `../../../skills/scorecard/SKILL.md`, which is outside the `.crate` package. `cargo package --list -p sc-cli` shows the missing file.
5. Repeat `cargo publish --dry-run` and verify the unpacked package before requesting approval to publish.

See Cargo's [path and registry dependency rules](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#multiple-locations) and the [cargo-binstall metadata reference](https://github.com/cargo-bins/cargo-binstall/blob/main/SUPPORT.md).
