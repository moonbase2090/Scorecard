#!/bin/sh
set -eu

if [ "$#" -ne 2 ]; then
  echo "usage: update-homebrew-formula-pr.sh VERSION SHA256SUMS" >&2
  exit 2
fi

version=${1#v}
branch="chore/homebrew-v$version"
tap_repo=moonbase2090/homebrew-scorecard
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

git config --local --unset-all http.https://github.com/.extraheader || true
gh auth setup-git
gh repo view "$tap_repo" --json name >/dev/null
gh repo clone "$tap_repo" "$tmp/tap"
base=$(gh repo view "$tap_repo" --json defaultBranchRef --jq '.defaultBranchRef.name // empty')
if [ -z "$base" ]; then
  echo "tap repository $tap_repo must have a default branch before formula updates can run" >&2
  exit 1
fi

git -C "$tmp/tap" fetch origin "+refs/heads/$base:refs/remotes/origin/$base"
if git -C "$tmp/tap" ls-remote --exit-code --heads origin "$branch" >/dev/null 2>&1; then
  git -C "$tmp/tap" fetch origin "+refs/heads/$branch:refs/remotes/origin/$branch"
  git -C "$tmp/tap" checkout -B "$branch" "origin/$branch"
else
  git -C "$tmp/tap" checkout -b "$branch" "origin/$base"
fi

python3 scripts/update-homebrew-formula.py "$version" "$2" "$tmp/tap/Formula/sc.rb"
git -C "$tmp/tap" add Formula/sc.rb
if ! git -C "$tmp/tap" diff --cached --quiet; then
  git -C "$tmp/tap" config user.name "MB2090"
  git -C "$tmp/tap" config user.email "322824348+mb2090@users.noreply.github.com"
  git -C "$tmp/tap" commit -m "Update Homebrew formula to $version"
  git -C "$tmp/tap" push --set-upstream origin "$branch"
fi

existing_pr=$(gh pr list --repo "$tap_repo" --base "$base" --head "$branch" --json url --jq '.[0].url // empty')
if [ -n "$existing_pr" ]; then
  echo "Updated Homebrew formula PR: $existing_pr"
elif git -C "$tmp/tap" diff "origin/$base" --quiet -- Formula/sc.rb; then
  echo "Homebrew formula already matches v$version; no PR is needed."
else
  gh pr create --repo "$tap_repo" --base "$base" --head "$branch" \
    --title "Update Homebrew formula to $version" \
    --body "## What
Updates the Homebrew formula to Scorecard v$version.

## Class
Leaf; this updates the formula for a published release.

## Proof
The formula URLs and SHA-256 checksums are generated from the published release assets' SHA256SUMS file.

## Risks / follow-ups
None."
fi
