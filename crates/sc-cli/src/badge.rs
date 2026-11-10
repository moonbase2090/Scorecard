// SPDX-License-Identifier: MIT
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
