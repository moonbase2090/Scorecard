// SPDX-License-Identifier: MIT
use sc_core::{Gate, Scorecard};
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
        s.contains("Overview") && s.contains("Findings [0]") && s.contains("Engines [1]")
    })?;
    terminal.wait_until(|s| s.contains("Selected gate: types") && s.contains("FAIL | 2 gates"))?;
    terminal.send(Key::Down)?;
    terminal.wait_until(|s| s.contains("Selected gate: tests") && s.contains("One test failed"))?;
    terminal.send(Key::Tab)?;
    terminal.wait_until(|s| s.contains("No findings in this report"))?;
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
