# Graphite in the terminal report (design)

Status: **proposal**. This document does not change the tool.

Audience: contributors changing the human terminal report.

## Problem

The human report in `crates/sc-cli/src/pretty.rs` paints with ANSI named colors and cuts overflow one Unicode scalar at a time. The progress line in `crates/sc-cli/src/progress.rs` is uncolored and does not cut a long step label. Graphite (`graphite-core`) already provides sRGB theme tokens, a WCAG contrast ratio, and `ellipsize`. The terminal report can use those three. It does not use Graphite pane, tab, legend, or layout APIs.

## Decisions

1. Default report color stays the ANSI palette (`green`, `red`, `blue`, `yellow`, bright black). The terminal theme owns that contrast.
2. An opt-in Graphite theme, off by default, paints truecolor from `tokens(ThemeVariant::Dark)` or `tokens(ThemeVariant::Light)`.
3. Depend on `graphite-core` by a full git revision. The crate sets `publish = false`, so there is no crates.io version to require.
4. Accept Graphite's MPL-2.0 license, the same license as Scorecard. Link the crate. Do not vendor or rewrite it. A separate helper, if one were ever required, may be MIT. None is required for this work.
5. Keep today's `NO_COLOR`, `CLICOLOR`, `CLICOLOR_FORCE`, and TTY rules for report color. The theme runs only when color is already on. `NO_COLOR` leaves the uncolored spinner visible.
6. Call `ellipsize` only after the pinned revision treats extended grapheme clusters as atomic. The default branch still walks scalars.
7. Keep the ASCII `~` cut when color is off. `ellipsize` always appends U+2026.
8. JSON, SARIF, HTML, Markdown, and MCP file and pipe output stay unchanged. With `--out`, stdout is the pretty report and follows the color and truncation rules below.

## Surfaces

### Progress animation

The spinner stays on stderr, braille frames, 80 ms tick, and the line rewrite `\r` plus clear-entire-line (`\x1b[2K`). It has no color. `NO_COLOR` leaves this uncolored spinner visible.

When the spinner is shown, cut the step label with `ellipsize` so the frame, label, separator, and elapsed time fit the terminal width. The budget is the column count minus the frame, the spaces, ` · `, and the elapsed field (`M:SS` or `H:MM:SS`).

`should_show` does not read `NO_COLOR`. The spinner stays hidden when stderr is not a TTY, when `--quiet` is set, when `TERM=dumb`, in CI (`CI`, `CONTINUOUS_INTEGRATION`, `GITHUB_ACTIONS`, `GITLAB_CI`, `JENKINS_URL`, `TEAMCITY_VERSION`, `TF_BUILD`), and when `SC_MCP` is set. Do not color the spinner.

### Report tables

`fit` is the only display-width cut in the human report. Call sites:

| Section | What is cut |
|---|---|
| Header | version, repo, pack, short SHA, state, scope |
| Gates | failure reason, after the id and kind columns |
| Scores | the whole score line, including the ASCII bar |
| CRAP | rest-of-tree and diff notes, the coverage note, the symbol (width 16), and the row |
| Test value | the mutation summary line |
| Generated files | each path and the suggested action |
| Findings | place and suggested action (the list stays capped at 8) |
| LLM | facts, verdict, and notes |

Padding stays. Gate ids stay padded to 16. The score bar stays `[` plus `#` and `.` plus `]`. Pass and fail marks stay `✓` / `✗` when color is on and `[ok]` / `[xx]` when color is off. The theme colors those marks. It does not change the glyphs.

`short_sha` keeps the first 7 bytes of a hex SHA. The HTML report's 12-character SHA is a separate abbreviation. Neither becomes an `ellipsize` call.

### Colors and themes

Default styles stay:

| Role | ANSI |
|---|---|
| Pass banner, passing gate | green, bold for the banner |
| Fail banner, failing gate, error severity | red, bold for the banner and severity |
| Report-only banner | blue, bold |
| Warning severity | yellow, bold |
| Scores and dim notes | bright black |
| Other severities | bold default |

The opt-in theme maps those roles onto token fields:

| Role | Token |
|---|---|
| Pass banner, passing gate | `working` |
| Fail banner, failing gate, error severity | `attention` |
| Warning severity | `unseen` |
| Scores and dim notes | `muted` |
| Other severities | `text_strong` |
| Report-only banner | `text` |

Paint with anstyle truecolor. For each painted color, `contrast_ratio(color, tokens.ground)` must be at least 4.5 in that variant. That check is for the sRGB pair. It does not apply to the default ANSI palette.

The report does not paint a background. The check assumes `tokens.ground`. It does not read the terminal's background.

The control is off by default. Values are `off`, `dark`, and `light`. The theme pull request names the flag or config key and updates `docs/reference/cli.md` in that same pull request.

### Truncation

`fit(text, width, unicode)` counts scalars. Callers pass `opts.color` as `unicode`. Over the width, it keeps `width - 1` scalars and appends `…` when that flag is true and `~` when it is false. Width is at least 1, so a cut at width 1 is the marker alone.

On the ellipsis path, replace that scalar walk with `ellipsize`:

- Metric: one cell per Unicode scalar (`chars().count()` as `f32`), the same measurement Graphite's own one-cell tests use. Face and pixel size are ignored. Pass `Face::Regular`.
- `max_w` is the column budget as `f32`.
- `ellipsize` returns an empty string when the ellipsis itself does not fit. Emit `…` in that case so width 1 still shows a cut.
- Trailing spaces before the ellipsis are dropped. That differs from today's `fit`, and it is accepted.
- The ASCII path stays a scalar cut plus `~`. Do not call `ellipsize` there.

A one-cell metric stops a cut inside a cluster once Graphite walks clusters. It still counts a ZWJ sequence as several cells. East Asian width and emoji display width are a later metric, not part of the truncation pull request.

## Dependency

```toml
graphite-core = { git = "https://github.com/moonbase2090/graphite", rev = "<full commit>" }
```

Pin a commit on Graphite's default branch. Do not pin a branch name. `graphite-core` 0.1.0 is edition 2021, `rust-version` 1.85, license MPL-2.0, `publish = false`. Scorecard's `rust-version` is 1.88, so the crate builds here.

The first code pull request adds the pin and a private adapter in `sc-cli`. A unit test calls `tokens` and `contrast_ratio`. Pretty output and the spinner do not call the adapter yet.

`ellipsize` on the default branch walks `chars()` and can split a combining mark, a flag, a ZWJ sequence, or a variation selector. A grapheme-cluster revision exists and is not on the default branch. The truncation pull request waits until that revision is the pinned commit. Token and contrast APIs are already on the default branch, so the pin pull request does not need to wait.

Every Scorecard crate sets `publish = false`. A git dependency is fine while that remains true. crates.io rejects git dependencies. A later publish of any Scorecard crate replaces this pin with a crates.io release of `graphite-core` first.

## `NO_COLOR` and non-TTY

`use_color` in `pretty.rs`:

1. `NO_COLOR` present (including an empty value) turns color off. This wins over `CLICOLOR_FORCE`.
2. When `CLICOLOR_FORCE` is set, that value decides color. `CLICOLOR` and the TTY are not consulted. `0` turns color off, including on a TTY. Any other value, including empty, turns color on, including when stdout is not a TTY.
3. When `CLICOLOR_FORCE` is unset, `CLICOLOR=0` turns color off.
4. Otherwise color follows whether stdout is a TTY.

`stdout_format`: an explicit `pretty`, `md`, `sarif`, `html`, `all`, or `json` wins. With no format, a TTY stdout uses pretty and any other stdout uses JSON.

Width is `COLUMNS` when it parses to at least 40, otherwise the Unix column count, otherwise 80. The rendered width is at least 40.

`use_color` does not read `TERM`. `TERM=dumb` hides the spinner and leaves report color to the list above. This design does not add `TERM=dumb` to the color decision.

Without `--out`, non-TTY stdout with no explicit format is JSON. That stream has no pretty color and no `fit`. Explicit pretty on a non-TTY is uncolored unless `CLICOLOR_FORCE` is a value other than `0`, and the cut uses `~`.

With `--out`, stdout is the pretty report in every environment. That includes a redirected stdout and a file written as JSON, SARIF, HTML, Markdown, or `all`. The file still receives the chosen format. Color on that stdout follows `use_color`. A redirected summary is uncolored unless `CLICOLOR_FORCE` is a value other than `0`, and the cut uses `~`.

The Graphite theme runs only when `use_color` is already true. `NO_COLOR` and `CLICOLOR_FORCE=0` both keep it off.

## Phased pull requests

| Phase | Change | Size | Class |
|---|---|---|---|
| 1 | This design document | small, one markdown file | docs |
| 2 | Pin `graphite-core`. Private `sc-cli` adapter and unit test. No pretty or spinner call. No CLI change. | small | trunk, because `Cargo.toml` and `Cargo.lock` change |
| 3 | Ellipsis path uses `ellipsize` after the grapheme-safe revision is pinned. The `~` path stays. | small | trunk if the pin moves, otherwise leaf (`pretty.rs` and its tests) |
| 4 | Opt-in theme, off by default, with the CLI reference updated in the same pull request | small to medium | trunk, public CLI or config |
| 5 | Ellipsize the spinner step when the spinner is shown. `NO_COLOR` still shows the uncolored spinner. Other show gates stay. | small | leaf (`progress.rs`) |

Phase 1 does not edit `CHANGELOG.md`. A later phase that changes what a person sees does. No phase bumps the version. New theme behavior ships off by default.

Phases 3 and 4 include a redirected `--out` summary in their proof: pretty text on a non-TTY stdout, a `~` cut when color is off, and the theme still off unless color is already on.

## Risks

- Pinning `ellipsize` before the grapheme revision splits clusters.
- Emitting U+2026 on the uncolored path puts a non-ASCII character in transcripts that today use `~`.
- Some terminals ignore truecolor. The theme is opt-in. The default stays ANSI.
- A 4.5 contrast check describes the token pair on `tokens.ground`. It does not describe ANSI named colors on the user's palette.
- A git revision bump edits the lockfile and is trunk.
- MPL-2.0 means Scorecard links `graphite-core` and does not vendor a copy.
- The crate is unpublished. A rewritten history can drop the pinned revision. Pin a full commit, not a branch.
- The lockfile will record the git dependency and its checksum.
- `fit`'s third argument is the color flag. The truncation pull request keeps that pairing.
- `NO_COLOR` leaves the uncolored spinner visible. Hiding it, or painting it, would change current behavior.
- The one-cell metric can count a ZWJ sequence as wider than a terminal cell.
- `ellipsize` can return an empty string. The adapter maps that to a single marker at width 1.
