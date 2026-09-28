# Check changes before each commit

`--diff HEAD` scores only what changed since the last commit, so it is fast enough for a git hook. From the repository root:

```bash doctest
cat > .git/hooks/pre-commit <<'EOF'
#!/bin/sh
exec sc analyze . --diff HEAD --format pretty
EOF
chmod +x .git/hooks/pre-commit
.git/hooks/pre-commit   # try it once
```

The hook blocks a commit when an enforced gate fails. Skip it once with `git commit --no-verify`.

For the [pre-commit](https://pre-commit.com) framework, add a local hook to `.pre-commit-config.yaml`:

```yaml
repos:
  - repo: local
    hooks:
      - id: sc
        name: sc
        entry: sc analyze . --diff HEAD --format pretty
        language: system
        pass_filenames: false
```
