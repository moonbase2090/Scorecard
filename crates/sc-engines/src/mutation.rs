// SPDX-License-Identifier: MPL-2.0
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use sc_core::{Finding, MutationSection};
use serde_json::Value;

use crate::command::{run_cmd, CommandError};

pub struct MutationOutcome {
    pub section: MutationSection,
    pub findings: Vec<Finding>,
    pub ran: bool,
    pub unavailable: Option<String>,
}

#[inline(never)]
pub fn run_mutation(
    root: &Path,
    mode: &str,
    base: Option<&str>,
    max_mutants: u32,
    budget: Duration,
    toolchain_pin: &str,
) -> MutationOutcome {
    let mode = mode.trim().to_ascii_lowercase();
    if mode.is_empty() || mode == "off" {
        return MutationOutcome {
            section: MutationSection::skipped(),
            findings: Vec::new(),
            ran: false,
            unavailable: None,
        };
    }
    if mode == "full" {
        return execute(root, None, max_mutants, budget, toolchain_pin);
    }
    let Some(base) = base else {
        return unavailable_outcome("mutation mode diff needs a git base; pass --diff or set one");
    };
    let patch = match git_diff(root, base) {
        Ok(patch) => patch,
        Err(err) => return unavailable_outcome(&err),
    };
    if patch.trim().is_empty() {
        let mut section = MutationSection::skipped();
        section.status = "empty-diff".into();
        return MutationOutcome {
            section,
            findings: Vec::new(),
            ran: false,
            unavailable: None,
        };
    }
    let patch_path = root.join(".sc").join("mutation.diff");
    if let Some(parent) = patch_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(err) = std::fs::write(&patch_path, patch) {
        return unavailable_outcome(&err.to_string());
    }
    execute(root, Some(&patch_path), max_mutants, budget, toolchain_pin)
}

fn execute(
    root: &Path,
    patch: Option<&Path>,
    max_mutants: u32,
    budget: Duration,
    toolchain_pin: &str,
) -> MutationOutcome {
    let pin = toolchain_pin.to_string();
    execute_with(
        root,
        patch,
        max_mutants,
        budget,
        move |root, args, timeout| cargo(root, args, timeout, &pin),
    )
}

#[inline(never)]
fn execute_with(
    root: &Path,
    patch: Option<&Path>,
    max_mutants: u32,
    budget: Duration,
    mut run: impl FnMut(&Path, &[String], Duration) -> Result<crate::command::Captured, CommandError>,
) -> MutationOutcome {
    let deadline = Instant::now() + budget;
    let left = match crate::command::budget_left(deadline) {
        Ok(left) => left,
        Err(_) => return unavailable_outcome("mutation budget exhausted"),
    };
    let mut args = vec!["mutants".to_string(), "--no-shuffle".to_string()];
    if let Some(patch) = patch {
        args.push("--in-diff".into());
        args.push(patch.to_string_lossy().to_string());
    }
    let list = run(root, &list_args(&args), left.min(Duration::from_secs(30)));
    let listed = match interpret_list(list) {
        Ok(count) => count,
        Err(outcome) => return outcome,
    };
    if listed > max_mutants as usize {
        return unavailable_outcome(&format!(
            "diff produces {listed} mutants, above max_mutants {max_mutants}"
        ));
    }
    let left = match crate::command::budget_left(deadline) {
        Ok(left) => left,
        Err(_) => return unavailable_outcome("mutation budget exhausted"),
    };
    finish_mutants(root, run(root, &args, left))
}

#[inline(never)]
fn interpret_list(
    list: Result<crate::command::Captured, CommandError>,
) -> Result<usize, MutationOutcome> {
    match list {
        Ok(captured) if captured.status.success() => Ok(parse_list_count(&captured.stdout)),
        Ok(captured) if tool_missing(&captured.stderr) || tool_missing(&captured.stdout) => {
            Err(unavailable_outcome("cargo-mutants is not installed"))
        }
        Ok(_) => Ok(0),
        Err(CommandError::NotFound) => Err(unavailable_outcome("cargo is not installed")),
        Err(CommandError::Timeout) => Err(unavailable_outcome("cargo mutants --list timed out")),
        Err(CommandError::Spawn(err)) => Err(unavailable_outcome(&err)),
    }
}

#[inline(never)]
fn finish_mutants(
    root: &Path,
    ran: Result<crate::command::Captured, CommandError>,
) -> MutationOutcome {
    match ran {
        Ok(captured) if tool_missing(&captured.stderr) || tool_missing(&captured.stdout) => {
            unavailable_outcome("cargo-mutants is not installed")
        }
        Ok(_) => read_outcomes(root),
        Err(CommandError::Timeout) => {
            let mut section = read_outcomes(root).section;
            section.status = "timeout".into();
            section.timeout = section.timeout.max(1);
            MutationOutcome {
                section,
                findings: Vec::new(),
                ran: false,
                unavailable: Some("cargo mutants timed out".into()),
            }
        }
        Err(CommandError::NotFound) => unavailable_outcome("cargo is not installed"),
        Err(CommandError::Spawn(err)) => unavailable_outcome(&err),
    }
}

fn list_args(args: &[String]) -> Vec<String> {
    let mut list = args.to_vec();
    list.push("--list".into());
    list.push("--json".into());
    list
}

fn cargo(
    root: &Path,
    args: &[String],
    timeout: Duration,
    toolchain_pin: &str,
) -> Result<crate::command::Captured, CommandError> {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(root)
        .args(args.iter().map(String::as_str))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .env("CARGO_TERM_COLOR", "never");
    let policy = crate::rust_toolchain::resolve(root, toolchain_pin);
    crate::rust_toolchain::apply(&mut cmd, &policy);
    run_cmd(&mut cmd, timeout)
}

fn git_diff(root: &Path, base: &str) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(root)
        .args(["diff", base])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    match run_cmd(&mut cmd, Duration::from_secs(10)) {
        Ok(captured) if captured.status.success() => Ok(captured.stdout),
        Ok(captured) => Err(captured.stderr.trim().to_string()),
        Err(err) => Err(err.message("git diff")),
    }
}

fn tool_missing(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("no such command") || lower.contains("command not found")
}

fn parse_list_count(stdout: &str) -> usize {
    let Ok(value) = serde_json::from_str::<Value>(stdout) else {
        return stdout
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count();
    };
    value.as_array().map(|items| items.len()).unwrap_or(0)
}

pub fn read_outcomes(root: &Path) -> MutationOutcome {
    let path = root.join("mutants.out").join("outcomes.json");
    let Ok(text) = std::fs::read_to_string(path) else {
        return unavailable_outcome("cargo mutants did not write mutants.out/outcomes.json");
    };
    parse_outcomes(&text)
}

pub fn parse_outcomes(text: &str) -> MutationOutcome {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return unavailable_outcome("mutants.out/outcomes.json is not json");
    };
    let Some(rows) = value.as_array() else {
        return unavailable_outcome("mutants.out/outcomes.json is not a list");
    };
    let mut killed = 0u64;
    let mut survived = 0u64;
    let mut timeout = 0u64;
    let mut unviable = 0u64;
    let mut findings = Vec::new();
    for row in rows {
        let summary = row
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase();
        match summary.as_str() {
            "caught" | "caughtmutant" => killed += 1,
            "timeout" | "timedout" => timeout += 1,
            "unviable" => unviable += 1,
            "missed" | "uncaught" => {
                survived += 1;
                findings.push(survivor_finding(row, findings.len()));
            }
            _ => {}
        }
    }
    let denom = killed + survived + timeout;
    let score = if denom == 0 {
        None
    } else {
        Some(killed as f64 / denom as f64)
    };
    MutationOutcome {
        section: MutationSection {
            status: "ran".into(),
            score,
            killed,
            survived,
            timeout,
            unviable,
        },
        findings,
        ran: true,
        unavailable: None,
    }
}

fn survivor_finding(row: &Value, index: usize) -> Finding {
    let scenario = row.get("scenario").unwrap_or(row);
    let description = scenario
        .get("summary")
        .or_else(|| scenario.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("mutant survived");
    let file = scenario
        .get("file")
        .or_else(|| row.get("file"))
        .and_then(Value::as_str)
        .unwrap_or(".");
    let line = scenario
        .get("line")
        .or_else(|| scenario.get("start_line"))
        .and_then(Value::as_u64)
        .map(|line| line as u32);
    let span = line.map(|start_line| sc_core::Span {
        start_line,
        start_col: 1,
        end_line: start_line,
        end_col: 1,
    });
    let location = line
        .map(|line| format!("{file}:{line}"))
        .unwrap_or_else(|| file.to_string());
    Finding {
        id: format!("mutation:{file}:{index}"),
        rule: "mutation.survivor".into(),
        engine: "mutation".into(),
        severity: "error".into(),
        file: file.to_string(),
        span,
        symbol: None,
        message: format!("mutant survived at {location}: {description}"),
        evidence: serde_json::json!({"summary": description}),
        suggested_action: Some("Add a test that kills this mutant".into()),
        disposition: String::new(),
    }
}

fn unavailable_outcome(message: &str) -> MutationOutcome {
    let mut section = MutationSection::skipped();
    section.status = "unavailable".into();
    MutationOutcome {
        section,
        findings: vec![Finding {
            id: "mutation:unavailable".into(),
            rule: "engine.unavailable".into(),
            engine: "mutation".into(),
            severity: "warning".into(),
            file: ".".into(),
            span: None,
            symbol: None,
            message: message.into(),
            evidence: serde_json::json!({}),
            suggested_action: Some("Install cargo-mutants or set mutation.mode to off".into()),
            disposition: String::new(),
        }],
        ran: false,
        unavailable: Some(message.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missed_mutant_is_a_survivor_and_timeout_is_not_a_kill() {
        let text = r#"[
          {"summary":"caught","scenario":{"summary":"replace a","file":"src/lib.rs"}},
          {"summary":"missed","scenario":{"summary":"replace b with 0","file":"src/lib.rs"}},
          {"summary":"timeout","scenario":{"summary":"loop","file":"src/lib.rs"}},
          {"summary":"unviable","scenario":{"summary":"nope","file":"src/lib.rs"}}
        ]"#;
        let outcome = parse_outcomes(text);
        assert_eq!(outcome.section.killed, 1);
        assert_eq!(outcome.section.survived, 1);
        assert_eq!(outcome.section.timeout, 1);
        assert_eq!(outcome.section.unviable, 1);
        assert_eq!(outcome.findings[0].rule, "mutation.survivor");
        assert!((outcome.section.score.unwrap() - (1.0 / 3.0)).abs() < 1e-9);
    }

    #[test]
    fn off_skips_and_diff_without_a_base_is_unavailable() {
        let root = Path::new(".");
        let off = run_mutation(root, "off", None, 1, Duration::from_secs(1), "");
        assert!(!off.ran);
        assert_eq!(off.section.status, "skipped");
        let missing = run_mutation(root, "diff", None, 1, Duration::from_secs(1), "");
        assert!(missing.unavailable.unwrap().contains("base"));
    }

    fn captured(ok: bool, stdout: &str, stderr: &str) -> crate::command::Captured {
        let status = if ok {
            std::process::Command::new("true").status().unwrap()
        } else {
            std::process::Command::new("false").status().unwrap()
        };
        crate::command::Captured {
            status,
            stdout: stdout.into(),
            stderr: stderr.into(),
            elapsed: Duration::from_millis(1),
        }
    }

    #[test]
    fn execute_reports_a_survivor_from_a_fake_cargo() {
        let root = std::env::temp_dir().join(format!("sc-mut-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("mutants.out")).unwrap();
        std::fs::write(
            root.join("mutants.out/outcomes.json"),
            r#"[{"summary":"missed","scenario":{"summary":"flip","file":"src/lib.rs"}}]"#,
        )
        .unwrap();
        let mut calls = 0;
        let outcome = execute_with(&root, None, 10, Duration::from_secs(5), |_, args, _| {
            calls += 1;
            if args.iter().any(|arg| arg == "--list") {
                Ok(captured(true, "[{}]", ""))
            } else {
                Ok(captured(true, "", ""))
            }
        });
        assert!(calls >= 2);
        assert_eq!(outcome.section.survived, 1);
        assert_eq!(outcome.findings[0].rule, "mutation.survivor");
        let too_many = execute_with(&root, None, 0, Duration::from_secs(5), |_, args, _| {
            if args.iter().any(|arg| arg == "--list") {
                Ok(captured(true, "[{},{}]", ""))
            } else {
                Ok(captured(true, "", ""))
            }
        });
        assert_eq!(too_many.section.status, "unavailable");
        let missing = execute_with(&root, None, 10, Duration::from_secs(5), |_, _, _| {
            Ok(captured(false, "", "no such command: mutants"))
        });
        assert!(missing.unavailable.unwrap().contains("not installed"));
        let timed = execute_with(&root, None, 10, Duration::ZERO, |_, _, _| {
            Err(CommandError::Timeout)
        });
        assert_eq!(timed.section.status, "unavailable");
        let patch = root.join("change.diff");
        std::fs::write(&patch, "diff\n").unwrap();
        let mut saw_diff = false;
        let ran_timeout = execute_with(
            &root,
            Some(&patch),
            10,
            Duration::from_secs(5),
            |_, args, _| {
                if args.iter().any(|arg| arg == "--in-diff") {
                    saw_diff = true;
                }
                if args.iter().any(|arg| arg == "--list") {
                    Ok(captured(true, "[]", ""))
                } else {
                    Err(CommandError::Timeout)
                }
            },
        );
        assert!(saw_diff);
        assert_eq!(ran_timeout.section.status, "timeout");
        let missing_cargo = execute_with(&root, None, 10, Duration::from_secs(5), |_, _, _| {
            Err(CommandError::NotFound)
        });
        assert!(missing_cargo.unavailable.unwrap().contains("cargo"));
        let listed_failed = execute_with(&root, None, 10, Duration::from_secs(5), |_, _, _| {
            Ok(captured(false, "", "build failed"))
        });
        assert_eq!(listed_failed.section.status, "ran");
        let spawned = execute_with(&root, None, 10, Duration::from_secs(5), |_, _, _| {
            Err(CommandError::Spawn("nope".into()))
        });
        assert!(spawned.unavailable.unwrap().contains("nope"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
