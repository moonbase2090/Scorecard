# Check changes before each commit

`--diff HEAD` scores only what changed since the last commit, so it is fast enough for a git hook. To use a Scorecard analysis hook instead of the repository formatting hook, run these commands from the repository root:

```bash doctest
cat > .git/hooks/pre-commit <<'EOF'
#!/bin/sh
exec sc analyze . --diff HEAD --format pretty
EOF
chmod +x .git/hooks/pre-commit
.git/hooks/pre-commit   # try it once
```

The analysis hook blocks a commit when an enforced gate fails. Its diff scope includes working files. Use the formatting hook below to check staged contents.

For the [pre-commit](https://pre-commit.com) framework, use Scorecard's published hook in `.pre-commit-config.yaml`:

```yaml
repos:
  - repo: https://github.com/moonbase2090/Scorecard
    rev: develop
    hooks:
      - id: sc
```

Both examples require `sc` on `PATH` and run the same analysis command as the manual hook above. For pre-commit, install the Git hook with `pre-commit install`. For repeatable installs, replace `develop` with a release tag that includes this hook.

For [Lefthook](https://lefthook.dev), add this to `lefthook.yml` and run `lefthook install`:

```yaml
pre-commit:
  commands:
    scorecard:
      run: sc analyze . --diff HEAD --format pretty
```

## Check staged formatting

Enable the checked-in formatting hook:

```sh
sh scripts/install-hooks.sh
```

This sets `core.hooksPath` to `.githooks`, replacing the active hooks directory
for this clone. The formatting hook checks a temporary copy of the index and
leaves your working files and staged contents unchanged. It needs Rust with
rustfmt for staged Rust changes and Node.js with npm for staged web changes.
The first web check downloads the pinned Biome version through `npx`.

For Rust, format the workspace and stage the corrected files:

```sh
cargo fmt --all
git add <formatted-files>
```

For JavaScript, TypeScript, and JSON, format the reported files and stage them:

```sh
npx --yes @biomejs/biome@2.6.2 format --write <files>
git add <formatted-files>
```

Retry the commit after staging. With partial staging, review `git diff --cached`
to confirm which formatted changes the commit includes. Rust checks cover only added, copied, modified, and renamed staged `.rs` files,
including files outside Cargo targets, without following unstaged child modules.
The Rust edition and rustfmt configuration come from the index. Web checks cover
added, copied, modified, and renamed
files with `.js`, `.mjs`, `.cjs`, `.jsx`, `.ts`, `.tsx`, `.json`, or `.jsonc`
extensions, using staged formatter configuration.

To verify the installed hook with real commits and both formatters, run:

```sh
python3 scripts/test-pre-commit.py
```
