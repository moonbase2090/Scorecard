# Homebrew formula maintenance

`Formula/sc.rb` installs the binaries from the signed macOS and Linux release archives. Its URLs and SHA-256 values must match the assets in the GitHub release.

Install by tapping the Scorecard repository with its explicit Git URL. After publishing a signed release, `.github/workflows/release.yml` generates an updated formula from `SHA256SUMS` and opens a pull request against `develop` in the Scorecard repository. The update branch is named `chore/homebrew-v<version>` so a release workflow retry reuses the same branch and pull request.

## Workflow credentials

The `publish` job uses a GitHub App to push the update branch and open its pull request. Configure these repository or organization Actions values:

- Variable `MOONBASE_AGENTS_APP_ID`: the App ID.
- Secret `MOONBASE_AGENTS_APP_PRIVATE_KEY`: the App private key.

Install the App on the Scorecard repository with `Contents: write` and `Pull requests: write` permissions. The workflow skips formula updates with a warning if either value is missing. The release assets are still published.

Review and merge each generated formula pull request through the normal Scorecard checks and review process.
