// SPDX-License-Identifier: MPL-2.0
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

/// Fixture crates share a `target/` directory. Keep analyzes serialized.
static FIXTURE_LOCK: Mutex<()> = Mutex::new(());

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn analyze(args: &[&str]) -> (i32, serde_json::Value, String, String) {
    let _guard = FIXTURE_LOCK.lock().unwrap_or_else(|err| err.into_inner());
    let output = Command::new(env!("CARGO_BIN_EXE_sc"))
        .current_dir(workspace())
        .arg("analyze")
        .args(args)
        .output()
        .expect("spawn sc");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(101);
    let value = serde_json::from_str(&stdout).unwrap_or_else(|err| {
        panic!("stdout was not JSON ({err})\nstdout:\n{stdout}\nstderr:\n{stderr}");
    });
    (code, value, stdout, stderr)
}

fn rules(card: &serde_json::Value) -> Vec<&str> {
    card["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|finding| finding["rule"].as_str())
        .collect()
}

#[test]
fn failing_test_fixture_exits_1_with_a_test_finding() {
    let (code, card, _, stderr) = analyze(&["testdata/failing_test"]);
    assert_eq!(code, 1, "stderr={stderr}\ncard={card}");
    assert_eq!(card["verdict"], "fail");
    let finding = card["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["rule"] == "test.failed")
        .unwrap_or_else(|| panic!("no test finding\n{card}"));
    assert!(finding["file"].as_str().unwrap().ends_with("src/lib.rs"));
    assert!(finding["symbol"].as_str().unwrap().contains("it_adds"));
    assert!(finding["span"].is_object());
    assert_eq!(finding["disposition"], "fix");
    assert!(card["runs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|run| run["engine"] == "tests"));
    assert!(card["gates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|gate| gate["id"] == "tests" && gate["pass"] == false));
    assert!(card["gates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|gate| gate["id"] == "types" && gate["pass"] == true));
}

#[test]
fn crap_untested_exits_1() {
    let (code, card, _, stderr) = analyze(&["testdata/crap_untested"]);
    assert_eq!(code, 1, "stderr={stderr}\ncard={card}");
    assert_eq!(card["verdict"], "fail");
    let finding = card["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["rule"] == "crap.over_threshold")
        .unwrap_or_else(|| panic!("missing crap finding\n{card}"));
    assert_eq!(finding["id"], "crap:src/lib.rs:classify");
    assert_eq!(finding["engine"], "crap");
    assert_eq!(finding["symbol"], "classify");
    assert!(finding["evidence"]["crap"].as_f64().unwrap() > 30.0);
    assert!(finding["evidence"]["cc"].as_u64().unwrap() >= 6);
    assert!(!card["crap"]["worst"].as_array().unwrap().is_empty());
    assert!(card["metrics"]["crap_over_threshold"].as_u64().unwrap() >= 1);
    assert_eq!(card["crap"]["threshold"], 30);
}

#[test]
fn crap_tested_exits_0() {
    let (code, card, _, stderr) = analyze(&["testdata/crap_tested"]);
    assert_eq!(code, 0, "stderr={stderr}\ncard={card}");
    assert_eq!(card["verdict"], "pass");
    assert!(
        !rules(&card).contains(&"crap.over_threshold"),
        "covered fixture still over threshold\n{card}"
    );
    let ran = card["engines_run"].as_array().unwrap();
    assert!(ran.iter().any(|engine| engine == "coverage"), "{card}");
    assert!(ran.iter().any(|engine| engine == "crap"), "{card}");
}

#[test]
fn workspace_src_is_scored_from_member_crates() {
    let (code, card, _, stderr) = analyze(&["testdata/workspace_src", "--budget-seconds", "120"]);
    assert_eq!(code, 0, "stderr={stderr}\ncard={card}");
    let paths: Vec<&str> = card["scope"]["paths"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|path| path.as_str())
        .collect();
    assert!(
        paths
            .iter()
            .any(|path| path.ends_with("crates/left/src/lib.rs")),
        "{paths:?}"
    );
    assert!(
        paths
            .iter()
            .any(|path| path.ends_with("crates/right/src/lib.rs")),
        "{paths:?}"
    );
    assert!(
        card["crap"]["worst"].as_array().unwrap().len() >= 2,
        "{card}"
    );
    assert!(
        card["metrics"]["crap_max"].as_f64().unwrap() > 0.0,
        "{card}"
    );
}

#[test]
fn good_crate_passes_and_has_the_scorecard_shape() {
    let out = std::env::temp_dir().join(format!("sc-good-{}.json", std::process::id()));
    let out_s = out.to_string_lossy().to_string();
    let (code, card, stdout, stderr) = analyze(&[
        "testdata/good_crate",
        "--budget-seconds",
        "120",
        "--out",
        &out_s,
    ]);
    assert_eq!(code, 0, "stderr={stderr}\ncard={card}");
    assert_eq!(card["verdict"], "pass");
    assert_eq!(card["version"], "0.1");
    assert_eq!(card["scope"]["mode"], "tree");
    for key in [
        "version",
        "id",
        "repo",
        "pack",
        "test_selection",
        "git",
        "scope",
        "verdict",
        "engines_run",
        "engines_skipped",
        "scores",
        "gates",
        "metrics",
        "crap",
        "mutation",
        "findings",
        "spec",
        "runs",
    ] {
        assert!(card.get(key).is_some(), "missing {key}");
    }
    assert_eq!(card["mutation"]["status"], "skipped");
    assert!(card["mutation"]["score"].is_null());
    assert!(card["spec"]["gaps"].as_array().unwrap().is_empty());
    assert_eq!(card["metrics"]["hallucinated_imports"], 0);
    assert_eq!(card["metrics"]["undeclared_dependencies"], 0);
    let skipped = card["engines_skipped"].as_array().unwrap();
    assert!(skipped.iter().any(|engine| engine == "llm"));
    assert!(skipped.iter().any(|engine| engine == "mutation"));
    for score in ["correctness", "efficiency", "maintainability", "security"] {
        let value = card["scores"][score].as_f64().unwrap();
        assert!((0.0..=1.0).contains(&value), "{score}={value}");
    }
    assert!(rules(&card)
        .iter()
        .all(|rule| *rule != "compile.error" && *rule != "test.failed"));
    let written = std::fs::read_to_string(&out).unwrap();
    assert_eq!(written.trim(), stdout.trim());
    let _ = std::fs::remove_file(&out);
}

#[test]
fn high_threshold_config_lets_crap_untested_pass() {
    let cfg = std::env::temp_dir().join(format!("sc-threshold-{}.toml", std::process::id()));
    std::fs::write(
        &cfg,
        "[gates]\nfail_on = [\"types\", \"tests\", \"crap\"]\ncrap_threshold = 10000\n",
    )
    .unwrap();
    let (code, card, _, stderr) =
        analyze(&["testdata/crap_untested", "--config", cfg.to_str().unwrap()]);
    assert_eq!(code, 0, "stderr={stderr}\ncard={card}");
    assert_eq!(card["verdict"], "pass");
    assert_eq!(card["crap"]["threshold"], 10000);
    assert!(!rules(&card).contains(&"crap.over_threshold"));
    let _ = std::fs::remove_file(&cfg);
}

#[test]
fn markdown_format_exits_0_on_good_crate() {
    let _guard = FIXTURE_LOCK.lock().unwrap_or_else(|err| err.into_inner());
    let output = Command::new(env!("CARGO_BIN_EXE_sc"))
        .current_dir(workspace())
        .args(["analyze", "testdata/good_crate", "--format", "md"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr={stderr}\nstdout={stdout}"
    );
    assert!(stdout.contains("**Verdict:** pass"), "{stdout}");
    assert!(stdout.contains("Worst CRAP"), "{stdout}");
}

#[test]
fn fake_dep_warns_on_hallucinated_import() {
    let (code, card, _, stderr) = analyze(&["testdata/fake_dep"]);
    assert_eq!(code, 0, "stderr={stderr}\ncard={card}");
    assert_eq!(card["verdict"], "pass");
    let finding = card["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["rule"] == "sca.undeclared_dependency")
        .expect("advisory dependency finding");
    assert_eq!(finding["severity"], "warning");
    assert_eq!(finding["disposition"], "ask");
    let message = finding["message"].as_str().unwrap();
    assert!(message.starts_with("Advisory:"), "{message}");
    assert!(!message.contains("Strongly"), "{message}");
    let reason = card["gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|gate| gate["id"] == "sca")
        .unwrap();
    assert_eq!(reason["enforced"], false);
    assert_eq!(reason["pass"], false);
    let reason = reason["reason"].as_str().unwrap();
    assert!(reason.contains("undeclared"), "{reason}");
    assert!(reason.contains("advisory"), "{reason}");
    assert!(card["metrics"]["undeclared_dependencies"].as_u64().unwrap() >= 1);
    assert_eq!(card["metrics"]["hallucinated_imports"], 0);
}

#[test]
fn local_mod_pub_use_is_not_hallucinated() {
    let (code, card, _, stderr) = analyze(&["testdata/local_mod"]);
    assert_eq!(code, 0, "stderr={stderr}\ncard={card}");
    let dependency: Vec<_> = card["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| {
            finding["rule"] == "sca.hallucinated_import"
                || finding["rule"] == "sca.undeclared_dependency"
        })
        .collect();
    assert!(dependency.is_empty(), "{dependency:?}");
    assert_eq!(card["metrics"]["hallucinated_imports"].as_u64().unwrap(), 0);
    assert_eq!(
        card["metrics"]["undeclared_dependencies"].as_u64().unwrap(),
        0
    );
}

#[test]
fn python_local_imports_are_not_dependency_findings() {
    let (code, card, _, stderr) = analyze(&["testdata/py_local_import"]);
    assert_eq!(code, 0, "stderr={stderr}\ncard={card}");
    let dependency: Vec<_> = rules(&card)
        .into_iter()
        .filter(|rule| rule.starts_with("sca."))
        .collect();
    assert!(dependency.is_empty(), "{dependency:?}\n{card}");
    assert_eq!(card["metrics"]["hallucinated_imports"], 0);
    assert_eq!(card["metrics"]["undeclared_dependencies"], 0);
    assert!(card["gates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|gate| { gate["id"] == "sca" && gate["pass"] == true && gate["enforced"] == false }));
}

#[test]
fn secret_token_fails_the_secrets_gate() {
    let (code, card, _, stderr) = analyze(&["testdata/secret_token"]);
    assert_eq!(code, 1, "stderr={stderr}\ncard={card}");
    assert!(rules(&card).contains(&"secrets.github_token"), "{card}");
    assert!(card["gates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|gate| gate["id"] == "secrets" && gate["pass"] == false));
}

#[test]
fn paths_file_limits_scope() {
    let list = std::env::temp_dir().join(format!("sc-paths-{}.txt", std::process::id()));
    std::fs::write(&list, "src/lib.rs\n").unwrap();
    let (code, card, _, stderr) =
        analyze(&["testdata/good_crate", "--paths", list.to_str().unwrap()]);
    assert_eq!(code, 0, "stderr={stderr}\ncard={card}");
    assert_eq!(card["scope"]["mode"], "paths");
    let _ = std::fs::remove_file(&list);
}

#[test]
fn pack_contract_passes_clean_trees_and_fails_secrets() {
    let passes = [
        "testdata/node_pack",
        "testdata/python_pack",
        "testdata/bash_pack",
        "testdata/go_pack",
        "testdata/java_pack",
        "testdata/csharp_pack",
        "testdata/php_pack",
        "testdata/cpp_pack",
        "testdata/web_site",
    ];
    for path in passes {
        let (code, card, _, stderr) = analyze(&[path]);
        assert_eq!(code, 0, "{path} stderr={stderr}\ncard={card}");
        assert_eq!(card["verdict"], "pass", "{path}");
        assert_eq!(card["test_selection"], "full-suite", "{path}");
        let crap_gate = card["gates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|gate| gate["id"] == "crap")
            .unwrap_or_else(|| panic!("{path} has no CRAP gate: {card}"));
        let coverage_missing = rules(&card)
            .iter()
            .any(|rule| matches!(*rule, "coverage.missing" | "coverage.unmatched"));
        if coverage_missing {
            assert_eq!(crap_gate["enforced"], false, "{path} {card}");
            assert_eq!(crap_gate["pass"], false, "{path} {card}");
        } else {
            assert_eq!(crap_gate["enforced"], true, "{path} {card}");
            assert_eq!(crap_gate["pass"], true, "{path} {card}");
        }
    }
    let (code, card, _, stderr) = analyze(&["testdata/command_pack", "--pack", "command"]);
    assert_eq!(code, 0, "stderr={stderr}\ncard={card}");
    assert_eq!(card["pack"], "command");

    let fails = [
        "testdata/node_pack_fail",
        "testdata/python_pack_fail",
        "testdata/bash_pack_fail",
        "testdata/go_pack_fail",
        "testdata/java_pack_fail",
        "testdata/csharp_pack_fail",
        "testdata/php_pack_fail",
        "testdata/cpp_pack_fail",
        "testdata/web_site_bad",
    ];
    for path in fails {
        let (code, card, _, stderr) = analyze(&[path]);
        assert_eq!(code, 1, "{path} stderr={stderr}\ncard={card}");
        assert!(
            rules(&card).iter().any(|rule| rule.starts_with("secrets.")),
            "{path} {card}"
        );
    }
    let (code, card, _, stderr) = analyze(&["testdata/command_pack_fail", "--pack", "command"]);
    assert_eq!(code, 1, "stderr={stderr}\ncard={card}");
    assert!(rules(&card).iter().any(|rule| rule.starts_with("secrets.")));
}

#[test]
fn web_pack_reports_markup_and_missing_files_with_locations() {
    let (code, card, _, stderr) = analyze(&["testdata/web_site"]);
    assert_eq!(code, 0, "stderr={stderr}\ncard={card}");
    assert_eq!(card["pack"], "web");
    assert!(card["gates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|gate| { gate["id"] == "html" && gate["pass"] == true && gate["enforced"] == true }));
    assert!(card["gates"].as_array().unwrap().iter().any(|gate| {
        gate["id"] == "links" && gate["pass"] == true && gate["enforced"] == false
    }));

    let (code, markdown, stderr) = analyze_raw(&["testdata/web_site_bad", "--format", "md"]);
    assert_eq!(code, 1, "stderr={stderr}\n{markdown}");
    assert!(
        markdown.contains("html.doctype") && markdown.contains("index.html:"),
        "{markdown}"
    );
    assert!(markdown.contains("links.missing"), "{markdown}");
    assert!(
        markdown.contains("html.unclosed") || markdown.contains("html.misnested"),
        "{markdown}"
    );
    let (pretty_code, pretty, pretty_err) =
        analyze_raw(&["testdata/web_site", "--format", "pretty"]);
    assert_eq!(pretty_code, 0, "{pretty_err}");
    assert_eq!(
        scrub_pretty(&pretty),
        include_str!("golden/web_site.pretty.txt")
    );
}

#[test]
fn a11y_stays_advisory_until_fail_on_names_it() {
    let (code, card, _, stderr) = analyze(&["testdata/a11y_page"]);
    assert_eq!(code, 0, "stderr={stderr}\ncard={card}");
    assert!(
        !rules(&card).iter().any(|rule| rule.starts_with("a11y.")),
        "{card}"
    );
    let (code, card, _, stderr) = analyze(&["testdata/a11y_page_bad", "--fail-on", ""]);
    assert_eq!(code, 0, "stderr={stderr}\ncard={card}");
    assert!(rules(&card).contains(&"a11y.img-alt"), "{card}");
    let (code, card, _, stderr) = analyze(&["testdata/a11y_page_bad", "--fail-on", "a11y"]);
    assert_eq!(code, 1, "stderr={stderr}\ncard={card}");
    assert!(card["gates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|gate| { gate["id"] == "a11y" && gate["pass"] == false && gate["enforced"] == true }));
}

#[test]
fn coverage_packs_record_a_hit() {
    let packs = [
        "testdata/cov_node",
        "testdata/cov_bash",
        "testdata/cov_java",
        "testdata/cov_csharp",
        "testdata/cov_php",
        "testdata/cov_cpp",
    ];
    for path in packs {
        let (code, card, _, stderr) = analyze(&[path, "--budget-seconds", "600"]);
        assert_eq!(code, 0, "{path} stderr={stderr}\ncard={card}");
        assert!(
            card["engines_run"]
                .as_array()
                .unwrap()
                .iter()
                .any(|engine| engine == "coverage"),
            "{path} did not read a coverage report\n{card}"
        );
        let cov = card["crap"]["worst"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["symbol"] == "choose")
            .and_then(|row| row["coverage"].as_f64())
            .unwrap_or(0.0);
        assert!(cov > 0.0, "{path} choose coverage is {cov}\n{card}");
    }
}

#[test]
fn missing_path_exits_2() {
    let (code, card, _, _) = analyze(&["testdata/does-not-exist"]);
    assert_eq!(code, 2);
    assert_eq!(card["verdict"], "fail");
    assert!(rules(&card).contains(&"engine.unavailable"));
}

#[test]
fn pretty_good_crate_matches_the_golden() {
    let (code, stdout, stderr) = analyze_raw(&[
        "testdata/good_crate",
        "--format",
        "pretty",
        "--budget-seconds",
        "180",
    ]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        scrub_pretty(&stdout),
        include_str!("golden/good_crate.pretty.txt")
    );
}

#[test]
fn pretty_failing_test_matches_the_golden() {
    let (code, stdout, stderr) = analyze_raw(&[
        "testdata/failing_test",
        "--format",
        "pretty",
        "--budget-seconds",
        "180",
    ]);
    assert_eq!(code, 1, "{stderr}");
    assert_eq!(
        scrub_pretty(&stdout),
        include_str!("golden/failing_test.pretty.txt")
    );
}

fn scrub_pretty(text: &str) -> String {
    let mut lines = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("sc ") {
            let mut parts: Vec<&str> = rest.split("  ").collect();
            if parts.len() >= 4 {
                parts[3] = "GITSHA STATE";
            }
            lines.push(format!("sc {}", parts.join("  ")));
        } else if line.starts_with("duration:") {
            lines.push("duration: DURs".to_string());
        } else {
            lines.push(line.to_string());
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

fn analyze_raw(args: &[&str]) -> (i32, String, String) {
    let _guard = FIXTURE_LOCK.lock().unwrap_or_else(|err| err.into_inner());
    let output = Command::new(env!("CARGO_BIN_EXE_sc"))
        .current_dir(workspace())
        .arg("analyze")
        .args(args)
        .output()
        .expect("spawn sc");
    let code = output.status.code().unwrap_or(101);
    (
        code,
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

#[test]
fn html_format_writes_a_self_contained_report() {
    let out = std::env::temp_dir().join(format!("sc-report-{}.html", std::process::id()));
    let out_s = out.to_string_lossy().to_string();
    let (code, stdout, stderr) =
        analyze_raw(&["testdata/good_crate", "--format", "html", "--out", &out_s]);
    assert_eq!(code, 0, "stderr={stderr}\n{stdout}");
    assert!(
        stdout.trim_start().starts_with("<!DOCTYPE html>"),
        "stdout is not html"
    );
    assert!(stdout.contains("PASS"), "missing verdict");
    assert!(stdout.contains("worst crap"), "missing crap section");
    let written = std::fs::read_to_string(&out).unwrap();
    assert_eq!(written.trim(), stdout.trim());
    for marker in ["unpkg", "cdn.", "http://", "https://"] {
        assert!(!written.contains(marker), "report fetches {marker}");
    }
    let _ = std::fs::remove_file(&out);
}

#[test]
fn all_format_writes_an_html_sibling() {
    let out = std::env::temp_dir().join(format!("sc-all-{}", std::process::id()));
    let out_s = out.to_string_lossy().to_string();
    let (code, stdout, stderr) =
        analyze_raw(&["testdata/good_crate", "--format", "all", "--out", &out_s]);
    assert_eq!(code, 0, "stderr={stderr}");
    // stdout stays JSON+Markdown, never HTML.
    assert!(stdout.trim_start().starts_with('{'));
    assert!(!stdout.contains("<!DOCTYPE html>"));
    let html = std::fs::read_to_string(out.with_extension("html")).unwrap();
    assert!(html.contains("<!DOCTYPE html>"));
    for ext in ["json", "md", "sarif", "html"] {
        let _ = std::fs::remove_file(out.with_extension(ext));
    }
}
