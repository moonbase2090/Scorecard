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
The Findings tab currently shows only its count; use `sc analyze --format pretty` to read findings.

Use an interactive terminal of at least 60 columns and 12 rows. Reopen the browser after
resizing. For redirected output, use `sc analyze --format json`.

To verify the browser from the source checkout, run:

```sh
bash scripts/verify-tui.sh
```

The termlens tests spawn the real binary in a PTY and check navigation, report errors,
small terminals, and exit. Set `SC_TUI_CAPTURE` to a file path to save the rendered
navigation screen as text.
