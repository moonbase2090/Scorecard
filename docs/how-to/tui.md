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
small terminals, exit, 100,000-finding scrolling, and escaped OSC 8 targets. Fixed snapshots
check the visible findings at the start and end. Set `SC_TUI_CAPTURE` to a file path to save
the navigation screen, or `SC_FINDINGS_CAPTURE` to save the large-report scroll screen as text.
