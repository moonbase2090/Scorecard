#!/bin/sh
set -eu

if [ "$#" -ne 2 ]; then
  echo "usage: update-homebrew-formula-pr.sh VERSION SHA256SUMS" >&2
  exit 2
fi

version=${1#v}
branch="chore/homebrew-v$version"

git config --local --unset-all http.https://github.com/.extraheader || true
gh auth setup-git
git fetch origin develop
if git ls-remote --exit-code --heads origin "$branch" >/dev/null 2>&1; then
  git fetch origin "+refs/heads/$branch:refs/remotes/origin/$branch"
  git checkout -B "$branch" "origin/$branch"
else
  git checkout -B "$branch" origin/develop
fi

python3 scripts/update-homebrew-formula.py "$version" "$2" Formula/sc.rb
git add Formula/sc.rb
if ! git diff --cached --quiet; then
  git config user.name "MB2090"
  git config user.email "322824348+mb2090@users.noreply.github.com"
  git commit -m "Update Homebrew formula to $version"
  git push --set-upstream origin "$branch"
fi

existing_pr=$(gh pr list --repo "$GITHUB_REPOSITORY" --base develop --head "$branch" --json url --jq '.[0].url // empty')
if [ -n "$existing_pr" ]; then
  echo "Updated Homebrew formula PR: $existing_pr"
elif git diff origin/develop --quiet -- Formula/sc.rb; then
  echo "Homebrew formula already matches v$version; no PR is needed."
else
  gh pr create --repo "$GITHUB_REPOSITORY" --base develop --head "$branch" \
    --label trunk \
    --title "Update Homebrew formula to $version" \
    --body "## What
Updates the Homebrew formula to Scorecard v$version.

## Class
trunk; the formula changes the supported binary distribution path.

## Proof
The formula URLs and SHA-256 checksums are generated from the published release assets' SHA256SUMS file. The release workflow runs this generator after it publishes those assets.

## Risks / follow-ups
None."
fi
