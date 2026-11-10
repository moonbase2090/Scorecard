// SPDX-License-Identifier: MIT
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use sc_core::{overall_score, Grade, Scorecard, ANALYSIS_COMPLETE};
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
                eprintln!("sc: cannot write badge: {err}. Check the output destination and retry sc badge.");
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
    if card.analysis != ANALYSIS_COMPLETE {
        return Ok(json!({
            "schemaVersion": 1,
            "label": "scorecard",
            "message": "incomplete",
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
    use sc_core::ANALYSIS_INCOMPLETE;

    #[test]
    fn a_complete_report_grades_perfect_scores() {
        let mut card = Scorecard::skeleton("demo", 30);
        card.engines.engines_run.push("html".into());
        let badge = from_card(&card).unwrap();
        assert_eq!(badge["message"], "A (100/100)");
        assert_eq!(badge["color"], "brightgreen");
    }

    #[test]
    fn an_incomplete_report_does_not_grade_perfect_scores() {
        let mut card = Scorecard::skeleton("demo", 30);
        card.analysis = ANALYSIS_INCOMPLETE.into();
        card.engines.engines_run.push("html".into());
        card.measures.overall = Some(sc_core::Overall::Score(sc_core::OverallScore {
            score: 100,
            grade: Grade::A,
        }));
        let badge = from_card(&card).unwrap();
        assert_eq!(badge["message"], "incomplete");
        assert_eq!(badge["color"], "lightgrey");
        assert!(!badge["message"].as_str().unwrap().contains('/'));
    }
}
