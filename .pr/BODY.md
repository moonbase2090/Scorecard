## Summary

Implements #81 item: on pull requests (`--diff`), report the changed code plus a one-line count for the rest of the tree.

- `scope.tree_paths` records how many source paths the tree-scope run would cover.
- Terminal, markdown, and HTML print `N paths in this diff; M other paths in the tree` (short form in the terminal).
- The existing CRAP diff-baseline line is unchanged.

## Test plan

- [x] `cargo test -p sc-core --lib`
- [x] `cargo test -p sc-cli --lib` (includes `rest_of_tree` / `html_rest_of_tree` / `markdown_rest_of_tree`)
- [x] Real `sc analyze . --diff HEAD~3 --format pretty` shows the rest-of-tree line

## Proof

SHA: `dda42df7f5f4e0b84c462bd079dfb922487b200b`

Before (develop, `--diff` worst-crap header):

```text
worst crap  threshold 30
  diff scope: 1 over threshold here; tree count: a run without --diff
```

After (this branch):

```text
worst crap  threshold 30
  9 paths in this diff; 34 other in the tree
  diff scope: 1 over threshold here; tree count: a run without --diff
```

## Intent

PR reports should lead with changed code and state how much of the tree was left outside the diff, without re-scoring the whole tree.

