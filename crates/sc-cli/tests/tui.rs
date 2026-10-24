// SPDX-License-Identifier: MIT
use sc_core::{Finding, Gate, Scorecard};
use std::process::Command;
use std::time::Duration;
use termlens::{Key, Terminal};

struct Project(std::path::PathBuf);
impl Project {
    fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("sc-tui-{label}-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".sc")).unwrap();
        Self(dir)
    }
    fn report(&self) {
        let mut card = Scorecard::skeleton("demo", 30);
        card.verdict = "fail".into();
        card.measures.gates = vec![
            Gate {
                id: "types".into(),
                pass: true,
                enforced: true,
                reason: None,
            },
            Gate {
                id: "tests".into(),
                pass: false,
                enforced: true,
                reason: Some("One test failed".into()),
            },
        ];
        card.measures.gates.push(Gate {
            id: "sca".into(),
            pass: false,
            enforced: false,
            reason: Some("Advisory dependency finding".into()),
        });
        card.sections.findings.push(Finding {
            id: "finding-1".into(),
            rule: "tests.failed".into(),
            engine: "tests".into(),
            severity: "error".into(),
            file: "src/lib.rs".into(),
            span: None,
            symbol: None,
            message: "One test failed".into(),
            evidence: serde_json::Value::Null,
            suggested_action: None,
            disposition: "fix".into(),
        });
        card.engines.engines_run = vec!["compile".into()];
        std::fs::write(
            self.0.join(".sc/last-scorecard.json"),
            serde_json::to_vec(&card).unwrap(),
        )
        .unwrap();
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn tui_help_describes_saved_report() {
    let out = Command::new(env!("CARGO_BIN_EXE_sc"))
        .args(["tui", "--help"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("saved scorecard"));
}

#[test]
fn tui_rejects_piped_output_without_escapes() {
    let out = Command::new(env!("CARGO_BIN_EXE_sc"))
        .arg("tui")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("interactive terminal"), "{err}");
    assert!(err.contains("sc analyze --format json"), "{err}");
    assert!(out.stdout.is_empty());
    assert!(!out.stderr.contains(&0x1b));
}

#[test]
fn tui_navigates_gates_tabs_and_quits() -> termlens::Result<()> {
    let project = Project::new("navigation");
    project.report();
    let mut terminal = Terminal::builder()
        .size(100, 24)
        .env_clear()
        .timeout(Duration::from_secs(5))
        .arg("tui")
        .arg(&project.0)
        .spawn(env!("CARGO_BIN_EXE_sc"))?;
    terminal.wait_until(|s| {
        s.contains("Overview") && s.contains("Findings [1]") && s.contains("Engines [1]")
    })?;
    terminal.wait_until(|s| s.contains("Selected gate: types") && s.contains("FAIL | 3 gates"))?;
    if let Some(path) = std::env::var_os("SC_TUI_CAPTURE") {
        std::fs::write(path, terminal.screen().text()).unwrap();
    }
    terminal.send(Key::Down)?;
    terminal.wait_until(|s| s.contains("Selected gate: tests") && s.contains("One test failed"))?;
    terminal.send(Key::End)?;
    terminal.wait_until(|s| s.contains("Selected gate: sca") && s.contains("Result: REPORT"))?;
    terminal.send(Key::Home)?;
    terminal.wait_until(|s| s.contains("Selected gate: types"))?;
    terminal.send(Key::Tab)?;
    terminal.wait_until(|s| s.contains("tests.failed") && s.contains("1/1 | PgUp/PgDn"))?;
    terminal.send(Key::Tab)?;
    terminal.wait_until(|s| s.contains("Ran: compile"))?;
    terminal.send(Key::Char('q'))?;
    assert!(terminal.wait_exit()?.success());
    Ok(())
}

#[test]
fn tui_missing_report_gives_recovery_command() -> termlens::Result<()> {
    let project = Project::new("missing");
    let mut terminal = Terminal::builder()
        .size(100, 24)
        .env_clear()
        .timeout(Duration::from_secs(5))
        .arg("tui")
        .arg(&project.0)
        .spawn(env!("CARGO_BIN_EXE_sc"))?;
    let status = terminal.wait_exit()?;
    assert_eq!(status.code(), Some(2));
    assert!(terminal.screen().contains("sc analyze"));
    assert!(terminal.screen().contains("saved scorecard"));
    Ok(())
}

#[test]
fn tui_handles_empty_reports_and_backward_navigation() -> termlens::Result<()> {
    let project = Project::new("empty");
    let card = Scorecard::skeleton("demo", 30);
    std::fs::write(
        project.0.join(".sc/last-scorecard.json"),
        serde_json::to_vec(&card).unwrap(),
    )
    .unwrap();
    let mut terminal = Terminal::builder()
        .size(80, 24)
        .env_clear()
        .timeout(Duration::from_secs(5))
        .arg("tui")
        .arg(&project.0)
        .spawn(env!("CARGO_BIN_EXE_sc"))?;
    terminal.wait_until(|s| s.contains("No gates in this report"))?;
    terminal.send(Key::Down)?;
    terminal.send(Key::Up)?;
    terminal.send(Key::BackTab)?;
    terminal.wait_until(|s| s.contains("Ran: none"))?;
    terminal.send(Key::Left)?;
    terminal.wait_until(|s| s.contains("No findings in this report"))?;
    terminal.send(Key::Char('q'))?;
    assert!(terminal.wait_exit()?.success());
    Ok(())
}

#[test]
fn tui_gives_a_size_hint_in_small_terminals() -> termlens::Result<()> {
    let project = Project::new("small");
    project.report();
    let mut terminal = Terminal::builder()
        .size(50, 10)
        .env_clear()
        .timeout(Duration::from_secs(5))
        .arg("tui")
        .arg(&project.0)
        .spawn(env!("CARGO_BIN_EXE_sc"))?;
    terminal.wait_until(|s| s.contains("at least 60x12"))?;
    terminal.send(Key::Ctrl('c'))?;
    assert!(terminal.wait_exit()?.success());
    Ok(())
}

#[test]
fn tui_rejects_malformed_saved_reports() -> termlens::Result<()> {
    let project = Project::new("malformed");
    std::fs::write(project.0.join(".sc/last-scorecard.json"), "{").unwrap();
    let mut terminal = Terminal::builder()
        .size(100, 24)
        .env_clear()
        .timeout(Duration::from_secs(5))
        .arg("tui")
        .arg(&project.0)
        .spawn(env!("CARGO_BIN_EXE_sc"))?;
    assert_eq!(terminal.wait_exit()?.code(), Some(2));
    assert!(terminal.screen().contains("Cannot open saved scorecard"));
    assert!(terminal.screen().contains("sc analyze"));
    Ok(())
}

#[test]
fn tui_renders_report_control_characters_as_text() -> termlens::Result<()> {
    let project = Project::new("controls");
    let mut card = Scorecard::skeleton("demo", 30);
    card.measures.gates.push(Gate {
        id: "\u{1b}[2Jtypes".into(),
        pass: true,
        enforced: true,
        reason: Some("first\nsecond".into()),
    });
    std::fs::write(
        project.0.join(".sc/last-scorecard.json"),
        serde_json::to_vec(&card).unwrap(),
    )
    .unwrap();
    let mut terminal = Terminal::builder()
        .size(100, 24)
        .env_clear()
        .timeout(Duration::from_secs(5))
        .arg("tui")
        .arg(&project.0)
        .spawn(env!("CARGO_BIN_EXE_sc"))?;
    terminal
        .wait_until(|s| s.contains("Selected gate:  [2Jtypes") && s.contains("first second"))?;
    assert!(terminal.screen().contains("Scorecard"));
    terminal.send(Key::Char('q'))?;
    assert!(terminal.wait_exit()?.success());
    Ok(())
}

fn finding(index: usize) -> Finding {
    Finding {
        id: format!("finding-{index}"),
        rule: "tests.failed".into(),
        engine: "tests".into(),
        severity: "error".into(),
        file: "src/lib.rs".into(),
        span: Some(sc_core::Span {
            start_line: index as u32 + 1,
            start_col: 1,
            end_line: index as u32 + 1,
            end_col: 2,
        }),
        symbol: None,
        message: format!("Finding number {}", index + 1),
        evidence: serde_json::Value::Null,
        suggested_action: None,
        disposition: "fix".into(),
    }
}

fn save(project: &Project, card: &Scorecard) {
    std::fs::write(
        project.0.join(".sc/last-scorecard.json"),
        serde_json::to_vec(card).unwrap(),
    )
    .unwrap();
}

#[test]
fn inline_saved_summary_is_plain_bounded_and_does_not_analyze() {
    let project = Project::new("inline");
    let mut card = Scorecard::skeleton("demo", 30);
    card.verdict = "fail".into();
    card.sections.findings = (0..100_000).map(finding).collect();
    save(&project, &card);
    let before = std::fs::read(project.0.join(".sc/last-scorecard.json")).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_sc"))
        .arg("tui")
        .arg(&project.0)
        .arg("--inline")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stderr.is_empty());
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "Scorecard FAIL | 0 gates | 100000 findings\nerror src/lib.rs:1 tests.failed\n  Finding number 1\nerror src/lib.rs:2 tests.failed\n  Finding number 2\nerror src/lib.rs:3 tests.failed\n  Finding number 3\nerror src/lib.rs:4 tests.failed\n  Finding number 4\nerror src/lib.rs:5 tests.failed\n  Finding number 5\n... 99995 more findings; open sc tui to browse\n");
    assert_eq!(
        std::fs::read(project.0.join(".sc/last-scorecard.json")).unwrap(),
        before
    );
}

#[test]
fn findings_scroll_through_one_hundred_thousand_real_rows() -> termlens::Result<()> {
    let project = Project::new("large");
    std::fs::create_dir(project.0.join("src")).unwrap();
    std::fs::write(project.0.join("src/lib.rs"), "// finding\n".repeat(100_000)).unwrap();
    let mut card = Scorecard::skeleton("demo", 30);
    card.sections.findings = (0..100_000).map(finding).collect();
    save(&project, &card);
    let mut terminal = Terminal::builder()
        .size(100, 24)
        .env_clear()
        .timeout(Duration::from_secs(5))
        .arg("tui")
        .arg(&project.0)
        .spawn(env!("CARGO_BIN_EXE_sc"))?;
    terminal.wait_until(|s| s.contains("Findings [100000]"))?;
    terminal.send(Key::Tab)?;
    terminal.wait_until(|s| s.contains("Finding number 1") && s.contains("1/100000 |"))?;
    terminal.send(Key::End)?;
    terminal
        .wait_until(|s| s.contains("Finding number 100000") && s.contains("100000/100000 |"))?;
    assert!(!terminal.screen().contains("src/lib.rs:1 "));
    terminal.send(Key::PageUp)?;
    terminal.wait_until(|s| s.contains("99984/100000 |") && s.contains("Finding number 99984"))?;
    terminal.send(Key::PageDown)?;
    terminal.wait_until(|s| s.contains("100000/100000 |"))?;
    terminal.send(Key::Home)?;
    terminal.wait_until(|s| s.contains("1/100000 |"))?;
    let mut transitions = Vec::new();
    for index in 2..=25 {
        let start = std::time::Instant::now();
        terminal.send(Key::Down)?;
        terminal.wait_until(|s| {
            s.contains(&format!("{index}/100000 |"))
                && s.contains(&format!("Finding number {index}"))
        })?;
        transitions.push(start.elapsed());
    }
    transitions.sort();
    eprintln!("100k findings: 24/24 observed Down transitions; input-to-screen median {:?}, max {:?}; report already loaded, actual file links enabled", transitions[12], transitions[23]);
    if let Some(path) = std::env::var_os("SC_FINDINGS_CAPTURE") {
        std::fs::write(path, terminal.screen().text()).unwrap();
    }
    terminal.send(Key::Char('q'))?;
    assert!(terminal.wait_exit()?.success());
    Ok(())
}

#[test]
fn full_screen_and_inline_emit_closed_escaped_file_line_links() -> termlens::Result<()> {
    let project = Project::new("links");
    let name = "space #%.rs";
    std::fs::write(project.0.join(name), "fn main() {}\n").unwrap();
    let mut card = Scorecard::skeleton("demo", 30);
    let mut item = finding(41);
    item.file = name.into();
    card.sections.findings.push(item);
    save(&project, &card);
    let suffix = "/space%20%23%25.rs#L42";
    for inline in [false, true] {
        let mut builder = Terminal::builder()
            .size(100, 24)
            .env_clear()
            .env("TERM", "xterm-256color")
            .timeout(Duration::from_secs(5))
            .arg("tui")
            .arg(&project.0);
        if inline {
            builder = builder.arg("--inline");
        }
        let mut terminal = builder.spawn(env!("CARGO_BIN_EXE_sc"))?;
        if inline {
            assert!(terminal.wait_exit()?.success());
        } else {
            terminal.wait_until(|s| s.contains("Overview"))?;
            terminal.send(Key::Tab)?;
            terminal.wait_until(|s| {
                s.links()
                    .iter()
                    .any(|link| link.uri().ends_with(suffix) && link.closed())
            })?;
        }
        let screen = terminal.screen();
        assert!(screen
            .links()
            .iter()
            .any(|link| link.uri().starts_with("file:///")
                && link.uri().ends_with(suffix)
                && link.label() == Some("space #%.rs:42")
                && link.closed()));
        if !inline {
            terminal.send(Key::Tab)?;
            terminal.wait_until(|s| s.contains("Ran: none"))?;
            terminal.send(Key::Char('q'))?;
            assert!(terminal.wait_exit()?.success());
        }
    }
    Ok(())
}

#[test]
fn inline_handles_missing_reports_without_terminal_setup() {
    let project = Project::new("inline-missing");
    let out = Command::new(env!("CARGO_BIN_EXE_sc"))
        .arg("tui")
        .arg(&project.0)
        .arg("--inline")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(!out.stderr.contains(&0x1b));
    assert!(String::from_utf8_lossy(&out.stderr).contains("Run sc analyze"));
}

#[test]
fn links_reject_outside_missing_and_control_paths_but_preserve_safe_labels() -> termlens::Result<()>
{
    let project = Project::new("link-boundary");
    let outside = Project::new("link-outside");
    std::fs::write(outside.0.join("outside.rs"), "outside").unwrap();
    std::fs::write(project.0.join("inside.rs"), "inside").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.0.join("outside.rs"), project.0.join("escape.rs")).unwrap();
    let mut card = Scorecard::skeleton("demo", 30);
    let paths = [
        outside.0.join("outside.rs").to_string_lossy().into_owned(),
        "../outside.rs".into(),
        "missing.rs".into(),
        "escape.rs".into(),
        "inside.rs\x1b]8;;https://example.invalid\x07".into(),
        "inside.rs".into(),
    ];
    for (index, path) in paths.into_iter().enumerate() {
        let mut item = finding(index);
        item.file = path;
        item.message = "first\nsecond\x1b[2J".into();
        card.sections.findings.push(item);
    }
    save(&project, &card);
    let mut terminal = Terminal::builder()
        .size(120, 24)
        .env_clear()
        .timeout(Duration::from_secs(5))
        .arg("tui")
        .arg(&project.0)
        .spawn(env!("CARGO_BIN_EXE_sc"))?;
    terminal.wait_until(|s| s.contains("Overview"))?;
    terminal.send(Key::Tab)?;
    terminal.wait_until(|s| {
        s.links()
            .iter()
            .any(|link| link.uri().ends_with("/inside.rs#L6"))
    })?;
    let screen = terminal.screen();
    assert!(screen
        .links()
        .iter()
        .all(|link| link.uri().ends_with("/inside.rs#L6") && link.closed()));
    assert!(screen.contains("first second [2J"));
    assert!(screen.contains("Scorecard"));
    terminal.send(Key::Char('q'))?;
    assert!(terminal.wait_exit()?.success());
    Ok(())
}

#[test]
fn inline_controls_are_plain_even_when_color_is_forced() {
    let project = Project::new("inline-controls");
    let mut card = Scorecard::skeleton("demo", 30);
    card.verdict = "fail\x1b[2J".into();
    let mut item = finding(0);
    item.file = "one\ntwo.rs".into();
    item.message = "first\nsecond\x1b]52;;c2VjcmV0\x07".into();
    card.sections.findings.push(item);
    save(&project, &card);
    let out = Command::new(env!("CARGO_BIN_EXE_sc"))
        .arg("tui")
        .arg(&project.0)
        .arg("--inline")
        .env("TERM", "xterm-256color")
        .env("CLICOLOR_FORCE", "1")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "Scorecard FAIL [2J | 0 gates | 1 findings\nerror one two.rs:1 tests.failed\n  first second ]52;;c2VjcmV0 \n");
}

#[test]
fn findings_redraw_changed_combining_graphemes() -> termlens::Result<()> {
    let project = Project::new("clusters");
    let mut card = Scorecard::skeleton("demo", 30);
    let mut first = finding(0);
    first.message = "e\u{301}".into();
    let mut second = finding(1);
    second.message = "o\u{308}".into();
    card.sections.findings = vec![first, second];
    save(&project, &card);
    let mut terminal = Terminal::builder()
        .size(100, 24)
        .env_clear()
        .timeout(Duration::from_secs(5))
        .arg("tui")
        .arg(&project.0)
        .spawn(env!("CARGO_BIN_EXE_sc"))?;
    terminal.wait_until(|s| s.contains("Overview"))?;
    terminal.send(Key::Tab)?;
    terminal.wait_until(|s| s.contains("e\u{301}"))?;
    terminal.send(Key::Down)?;
    terminal.wait_until(|s| s.contains("o\u{308}") && s.contains("2/2 |"))?;
    assert!(!terminal.screen().contains("e\u{301}"));
    terminal.send(Key::Char('q'))?;
    assert!(terminal.wait_exit()?.success());
    Ok(())
}
