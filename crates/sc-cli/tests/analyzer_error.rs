// SPDX-License-Identifier: MIT
use std::path::PathBuf;
use std::process::{Command, Output};

use sc_core::{Scorecard, Scores};
use serde_json::Value;

struct Project(PathBuf);

impl Project {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("sc-analyzer-error-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_sc"))
            .current_dir(&self.0)
            .env("SCORECARD_NO_AGENT_SKILLS", "1")
            .args(args)
            .output()
            .unwrap()
    }

    fn save(&self, card: &Scorecard) {
        std::fs::create_dir_all(self.0.join(".sc")).unwrap();
        std::fs::write(
            self.0.join(".sc/last-scorecard.json"),
            serde_json::to_string(card).unwrap(),
        )
        .unwrap();
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn json_stdout(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "stdout is not JSON ({err}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

fn letter_grade(message: &str) -> bool {
    let Some(grade) = message.chars().next() else {
        return false;
    };
    matches!(grade, 'A' | 'B' | 'C' | 'D' | 'F') && message.contains("/100")
}

#[test]
fn an_unfinished_run_is_marked_and_the_badge_refuses_a_letter() {
    let project = Project::new("empty");
    let output = project.run(&[
        "analyze", ".", "--format", "all", "--out", "report", "--llm", "off",
    ]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let pretty = String::from_utf8_lossy(&output.stdout);
    assert!(pretty.contains("ERROR\n"), "{pretty}");
    assert!(pretty.contains("no letter grade"), "{pretty}");
    assert!(pretty.contains("sc analyze ."), "{pretty}");
    assert!(!letter_grade(&pretty), "{pretty}");

    let card: Value =
        serde_json::from_str(&std::fs::read_to_string(project.0.join("report.json")).unwrap())
            .unwrap();
    assert_eq!(card["analyzer_error"], true, "{card}");
    assert_eq!(card["verdict"], "fail");
    let saved: Value = serde_json::from_str(
        &std::fs::read_to_string(project.0.join(".sc/last-scorecard.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(saved["analyzer_error"], true);

    let markdown = std::fs::read_to_string(project.0.join("report.md")).unwrap();
    assert!(markdown.contains("**Verdict:** error"));
    assert!(markdown.contains("no letter grade"));
    assert!(!markdown.contains("grade A"));
    let html = std::fs::read_to_string(project.0.join("report.html")).unwrap();
    assert!(html.contains("<span class=\"verdict error\">ERROR</span>"));
    assert!(html.contains(">error</span>"));
    assert!(!html.contains("grade A"));
    assert!(!html.contains("100/100"));
    let sarif: Value =
        serde_json::from_str(&std::fs::read_to_string(project.0.join("report.sarif")).unwrap())
            .unwrap();
    assert_eq!(sarif["runs"][0]["properties"]["analyzerError"], true);

    let badge = project.run(&["badge"]);
    assert_eq!(badge.status.code(), Some(0), "{badge:?}");
    let badge = json_stdout(&badge);
    assert_eq!(badge["message"], "error");
    assert_eq!(badge["label"], "scorecard");
    assert_eq!(badge["schemaVersion"], 1);
    assert!(!letter_grade(badge["message"].as_str().unwrap()));
}

#[test]
fn a_saved_perfect_score_does_not_keep_its_letter_after_an_analyzer_error() {
    let project = Project::new("stale");
    let mut card = Scorecard::skeleton("demo", 30);
    card.engines.engines_run.push("secrets".into());
    card.measures.scores = Scores::perfect();
    card.verdict = "pass".into();
    project.save(&card);

    let before = project.run(&["badge"]);
    assert_eq!(before.status.code(), Some(0), "{before:?}");
    assert_eq!(json_stdout(&before)["message"], "A (100/100)");

    let failed = project.run(&["analyze", ".", "--fail-on", "", "--format", "json"]);
    assert_eq!(failed.status.code(), Some(2), "{failed:?}");
    let card = json_stdout(&failed);
    assert_eq!(card["analyzer_error"], true);
    assert_eq!(card["verdict"], "fail");
    let saved: Value = serde_json::from_str(
        &std::fs::read_to_string(project.0.join(".sc/last-scorecard.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(saved["analyzer_error"], true);

    let after = project.run(&["badge"]);
    assert_eq!(after.status.code(), Some(0), "{after:?}");
    let after = json_stdout(&after);
    assert_eq!(after["message"], "error");
    assert_ne!(after["message"], "A (100/100)");
}

#[test]
fn a_finished_web_report_still_earns_a_letter_grade() {
    let project = Project::new("web");
    let site = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/web_site");
    let status = Command::new("cp")
        .args(["-R", site.to_str().unwrap(), project.0.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(status.success());
    let copied = project.0.join("web_site");
    let output = Command::new(env!("CARGO_BIN_EXE_sc"))
        .current_dir(&copied)
        .env("SCORECARD_NO_AGENT_SKILLS", "1")
        .args(["analyze", ".", "--format", "json", "--llm", "off"])
        .output()
        .unwrap();
    assert_ne!(output.status.code(), Some(2), "{output:?}");
    let card = json_stdout(&output);
    assert!(card.get("analyzer_error").is_none(), "{card}");
    assert_eq!(card["verdict"], "pass");
    let badge = Command::new(env!("CARGO_BIN_EXE_sc"))
        .current_dir(&copied)
        .env("SCORECARD_NO_AGENT_SKILLS", "1")
        .args(["badge"])
        .output()
        .unwrap();
    assert_eq!(badge.status.code(), Some(0), "{badge:?}");
    let message = json_stdout(&badge)["message"].as_str().unwrap().to_string();
    assert!(letter_grade(&message), "{message}");
}
