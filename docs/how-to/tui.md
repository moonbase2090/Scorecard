# Browse a saved analysis

Build Scorecard with the optional terminal browser:

```sh
cargo build --release -p sc-cli --features tui
```

From your project directory, save an analysis and open it with that build:

```sh
sc analyze .
sc tui .
```

Use `target/release/sc` from the source checkout if you have not installed your build on PATH.
Analysis can exit 1 for failed gates and still save the report. The browser shows those
results without rerunning tools. Run `sc analyze` again to refresh the saved report.

Select gates with Up/Down or `j`/`k`. Use Tab and Shift+Tab to move between Overview,
Findings, and Engines. The badges count saved findings and engines that ran. Press `q` to quit.
On Findings, Up/Down selects a row and shows its message below the table. Use PageUp
and PageDown to scroll a page, or Home and End to jump to the first and last findings.
The browser formats only visible rows, even for reports with 100,000 findings.

Click a file-and-line link in a terminal that supports OSC 8. The target must be an existing
file inside the project. Missing files and paths outside the project remain plain text.
Line numbers use `#L<line>` URI fragments, whose handling depends on the terminal.

To leave a compact summary in scrollback or send it to another command, run:

```sh
sc tui . --inline
```

Inline mode shows up to eight gates and five findings, then counts the rest. It reads no
input and leaves the terminal screen intact. Redirected inline output contains no terminal escapes.

Use an interactive terminal of at least 60 columns and 12 rows. Reopen the browser after
resizing. For redirected output, use `sc analyze --format json`.

To verify the browser from the source checkout, run:

```sh
bash scripts/verify-tui.sh
```

The termlens tests spawn the real binary in a PTY and check navigation, report errors,
small terminals, terminal restoration on quit, 100,000-finding scrolling, and escaped OSC 8 targets. Fixed snapshots
check the visible findings at the start and end. Set `SC_TUI_CAPTURE` to a file path to save
the navigation screen, or `SC_FINDINGS_CAPTURE` to save the large-report scroll screen as text.

The styled snapshots cover all three tabs, passing, failed, and advisory gate selection,
and the first and last findings at 60x12 and 100x24. Each snapshot includes text,
foreground, background, and attributes. The encoder output also passes a vt100 oracle.
Review each changed snapshot before accepting it; CI never updates snapshots.

Run the release-build frame budget on macOS or Linux with Python 3 installed:

```sh
bash scripts/verify-tui-perf.sh
```

The `tui-perf` CI job fails if any of 180 measured frames exceeds 16 ms. It runs
30 full repaints and 30 Home/End transitions at each of 100x24, 400x120, and
800x240, with 100,000 loaded findings and real file links. Each sample includes
input handling, grid rendering, diff encoding, link encoding, synchronized output,
and writing/flushing to a draining PTY. Every emitted frame is checked against a
vt100 oracle after timing. Full repaint samples start from a cleared screen. Report construction, JSON loading, terminal setup,
and oracle checks are outside the timed interval. This measures application frame
production and PTY delivery, not terminal-emulator painting or key-to-screen latency.
CI saves the maximum and byte count for every case as the `tui-perf` artifact.
The 8 ms dedicated-machine target is separate from the 16 ms shared-runner gate.

TUI remains an optional build feature. These checks do not enable it in release
artifacts or authorize a version bump or release.
