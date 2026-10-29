# Homebrew formula maintenance

`Formula/sc.rb` in `moonbase2090/homebrew-scorecard` installs the binaries from the signed macOS and Linux release archives. Its URLs and SHA-256 values must match the assets in the GitHub release.

Install using the short tap name:

```bash
brew tap moonbase2090/scorecard
brew install moonbase2090/scorecard/sc
```

After publishing a signed release, `.github/workflows/release.yml` generates an updated formula from `SHA256SUMS` and opens a pull request against the default branch of `moonbase2090/homebrew-scorecard`. The update branch is named `chore/homebrew-v<version>` so a release workflow retry reuses the same branch and pull request.

## Workflow credentials

The `publish` job uses a GitHub App to push the update branch and open its pull request. Configure these repository or organization Actions values:

- Variable `MOONBASE_AGENTS_APP_ID`: the App ID.
- Secret `MOONBASE_AGENTS_APP_PRIVATE_KEY`: the App private key.

Install the App on both the Scorecard and `homebrew-scorecard` repositories with `Contents: write` and `Pull requests: write` permissions. The workflow skips formula updates with a warning if either value is missing. The release assets are still published.

Review and merge each generated formula pull request through the normal Scorecard checks and review process.
