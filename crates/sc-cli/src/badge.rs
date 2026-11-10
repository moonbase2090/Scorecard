// SPDX-License-Identifier: MIT
//! Shields.io endpoint JSON for a saved scorecard.
//!
//! A finished run gets an A–F grade from the dimension scores. An analyzer
//! error is an explicit `error` badge with no letter, even when every
//! dimension is still 1.0.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use sc_core::{overall_score, Grade, Scorecard};
use serde_json::{json, Value};

pub fn run(input: &Path) -> i32 {
    let badge = fs::read_to_string(input)
        .map_err(|err| format!("cannot read report {}: {err}", input.display()))
        .and_then(|text| {
            serde_json::from_str::<Scorecard>(&text)
                .map_err(|err| format!("invalid report {}: {err}", input.display()))
        })
        .and_then(|card| from_card(&card));
    match badge {
        Ok(badge) => match writeln!(io::stdout().lock(), "{badge}") {
            Ok(()) => 0,
            Err(err) => {
                eprintln!(
                    "sc: cannot write badge: {err}. Check the output destination and retry sc badge."
                );
                2
            }
        },
        Err(err) => {
            eprintln!(
                "sc: {err}. Run sc analyze . in the project, then retry sc badge --input {}.",
                input.display()
            );
            2
        }
    }
}

fn from_card(card: &Scorecard) -> Result<Value, String> {
    if card.analyzer_error {
        return Ok(json!({
            "schemaVersion": 1,
            "label": "scorecard",
            "message": "error",
            "color": "lightgrey",
        }));
    }
    if card.engines.engines_run.is_empty() {
        return Err("report has no completed engines; no overall score is available".into());
    }
    let overall = overall_score(&card.measures.scores)
        .ok_or("report dimension scores must all be finite numbers in 0..=1")?;
    let color = match overall.grade {
        Grade::A => "brightgreen",
        Grade::B => "green",
        Grade::C => "yellow",
        Grade::D => "orange",
        Grade::F => "red",
    };
    Ok(json!({
        "schemaVersion": 1,
        "label": "scorecard",
        "message": format!("{} ({}/100)", overall.grade, overall.score),
        "color": color,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::Scores;

    fn graded(dimension: f64) -> Scorecard {
        let mut card = Scorecard::skeleton("demo", 30);
        card.engines.engines_run.push("secrets".into());
        card.measures.scores = Scores {
            correctness: dimension,
            efficiency: dimension,
            maintainability: dimension,
            security: dimension,
            a11y: dimension,
        };
        card
    }

    #[test]
    fn a_finished_run_gets_a_letter_and_an_analyzer_error_does_not() {
        let perfect = graded(1.0);
        assert_eq!(from_card(&perfect).unwrap()["message"], "A (100/100)");
        let mut errored = perfect;
        errored.analyzer_error = true;
        errored.measures.scores.correctness = 1.5;
        errored.engines.engines_run.clear();
        let badge = from_card(&errored).unwrap();
        assert_eq!(
            badge,
            json!({"schemaVersion":1,"label":"scorecard","message":"error","color":"lightgrey"})
        );
        let message = badge["message"].as_str().unwrap();
        assert!(
            !message.chars().any(|ch| matches!(ch, 'A'..='F')),
            "{message}"
        );
    }

    #[test]
    fn letter_colors_follow_the_grade() {
        for (dimension, message, color) in [
            (1.0, "A (100/100)", "brightgreen"),
            (0.85, "B (85/100)", "green"),
            (0.75, "C (75/100)", "yellow"),
            (0.65, "D (65/100)", "orange"),
            (0.1, "F (10/100)", "red"),
        ] {
            let badge = from_card(&graded(dimension)).unwrap();
            assert_eq!(badge["message"], message, "{dimension}");
            assert_eq!(badge["color"], color, "{dimension}");
        }
    }
}
