// SPDX-License-Identifier: MIT
use std::path::PathBuf;
use std::process::{Command, Output};

use sc_core::{Grade, OverallScore, Scorecard, Scores};
use serde_json::{json, Value};

struct Project(PathBuf);

impl Project {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("sc-overall-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("index.html"), "<!doctype html><html lang=\"en\"><head><meta name=\"viewport\" content=\"width=device-width\"><title>Example</title></head><body><main><h1>Example</h1></main></body></html>").unwrap();
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

    fn json(&self, file: &str) -> Value {
        serde_json::from_str(&std::fs::read_to_string(self.0.join(file)).unwrap()).unwrap()
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

#[test]
fn overall_is_opt_in_and_agrees_across_saved_and_rendered_reports() {
    let project = Project::new("reports");
    let plain = project.run(&["analyze", ".", "--format", "json"]);
    assert_eq!(plain.status.code(), Some(0), "{plain:?}");
    let plain: Value = serde_json::from_slice(&plain.stdout).unwrap();
    assert!(plain.get("overall").is_none());
    assert!(project
        .json(".sc/last-scorecard.json")
        .get("overall")
        .is_none());
    let badge = project.run(&["badge"]);
    assert_eq!(badge.status.code(), Some(0), "{badge:?}");
    let badge: Value = serde_json::from_slice(&badge.stdout).unwrap();
    assert_eq!(badge["message"], "A (100/100)");

    let enabled = project.run(&[
        "analyze",
        ".",
        "--overall-score",
        "--format",
        "all",
        "--out",
        "report",
    ]);
    assert_eq!(enabled.status.code(), Some(0), "{enabled:?}");
    assert!(String::from_utf8_lossy(&enabled.stdout).contains("overall          100/100  grade A"));
    for file in ["report.json", ".sc/last-scorecard.json"] {
        let report = project.json(file);
        assert_eq!(report["overall"], json!({"score":100,"grade":"A"}));
        assert_eq!(report["verdict"], "pass");
    }
    assert_eq!(
        project.json("report.sarif")["runs"][0]["properties"]["overall"],
        json!({"score":100,"grade":"A"})
    );
    assert!(std::fs::read_to_string(project.0.join("report.md"))
        .unwrap()
        .contains("Overall: 100/100, grade A."));
    assert!(std::fs::read_to_string(project.0.join("report.html"))
        .unwrap()
        .contains("100/100, grade A</td>"));
}

#[test]
fn config_enables_overall_from_aggregate_multi_pack_dimensions() {
    let project = Project::new("multi");
    std::fs::create_dir(project.0.join("child")).unwrap();
    std::fs::rename(
        project.0.join("index.html"),
        project.0.join("child/index.html"),
    )
    .unwrap();
    std::fs::create_dir(project.0.join("other")).unwrap();
    std::fs::write(project.0.join("other/index.html"), "<!doctype html><html lang=\"en\"><head><title>Missing viewport</title></head><body><main><h1>Example</h1></main></body></html>").unwrap();
    std::fs::write(
        project.0.join("analyzer.toml"),
        "pack = \"multi\"\noverall_score = true\n[gates]\nfail_on = [\"html\"]\n",
    )
    .unwrap();
    let output = project.run(&["analyze", "."]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let card: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(card["pack"], "multi");
    assert_eq!(card["scores"]["correctness"], 0.75);
    assert_eq!(card["overall"], json!({"score":95,"grade":"A"}));
    assert_eq!(
        project.json(".sc/last-scorecard.json")["overall"],
        json!({"score":95,"grade":"A"})
    );
}

#[test]
fn analyzer_errors_have_no_overall_score() {
    let project = Project::new("error");
    let output = project.run(&["analyze", "missing", "--overall-score"]);
    assert_eq!(output.status.code(), Some(2));
    let card: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(card.get("overall").is_none());
    std::fs::write(
        project.0.join("analyzer.toml"),
        "pack = \"unknown\"\noverall_score = true\n",
    )
    .unwrap();
    let output = project.run(&["analyze", "."]);
    assert_eq!(output.status.code(), Some(2));
    let card: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(card.get("overall").is_none());
}

#[test]
fn badge_recomputes_saved_dimensions_and_does_not_change_a_failing_verdict() {
    let project = Project::new("badge");
    let mut card = Scorecard::skeleton("demo", 30);
    card.engines.engines_run.push("secrets".into());
    // Ignore a stale cached summary. The gate verdict is independent of the grade.
    card.measures.overall = Some(OverallScore {
        score: 0,
        grade: Grade::F,
    });
    for (dimension, message, color) in [
        (1.0, "A (100/100)", "brightgreen"),
        (0.85, "B (85/100)", "green"),
        (0.75, "C (75/100)", "yellow"),
        (0.65, "D (65/100)", "orange"),
        (0.1, "F (10/100)", "red"),
    ] {
        card.measures.scores = Scores {
            correctness: dimension,
            efficiency: dimension,
            maintainability: dimension,
            security: dimension,
            a11y: dimension,
        };
        project.save(&card);
        let output = project.run(&["badge"]);
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        let badge: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            badge,
            json!({"schemaVersion":1,"label":"scorecard","message":message,"color":color})
        );
        assert_eq!(project.json(".sc/last-scorecard.json")["verdict"], "fail");
    }
}

#[test]
fn badge_errors_explain_how_to_create_a_valid_report() {
    let project = Project::new("badge-errors");
    for text in [None, Some("not JSON"), Some("{}")] {
        if let Some(text) = text {
            std::fs::write(project.0.join("saved.json"), text).unwrap();
        }
        let output = project.run(&["badge", "--input", "saved.json"]);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("Run sc analyze ."));
    }
    let mut card = Scorecard::skeleton("demo", 30);
    project.save(&card);
    let output = project.run(&["badge"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("no completed engines"));
    card.engines.engines_run.push("secrets".into());
    card.measures.scores.correctness = 1.5;
    project.save(&card);
    let output = project.run(&["badge"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("dimension scores"));
}
