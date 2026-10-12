// SPDX-License-Identifier: MIT
use std::process::Command;

#[test]
fn excluding_the_whole_tree_reports_an_incomplete_secrets_scan() {
    let root = std::env::temp_dir().join(format!("sc-empty-scope-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("index.html"),
        "<!doctype html><html lang=\"en\"><head><meta name=\"viewport\" content=\"width=device-width\"><title>Test</title></head><body><main>Test</main></body></html>",
    )
    .unwrap();
    for in_git in [false, true] {
        if in_git {
            assert!(Command::new("git")
                .args(["init", "-q"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success());
        }
        std::fs::write(
            root.join("analyzer.toml"),
            "pack = \"web\"\n[scope]\nexclude = [\"/**\"]\n",
        )
        .unwrap();
        for (fail_on, code) in [("secrets", 1), ("none", 0)] {
            let output = Command::new(env!("CARGO_BIN_EXE_sc"))
                .current_dir(&root)
                .args(["analyze", ".", "--format", "json", "--fail-on", fail_on])
                .output()
                .unwrap();
            let card: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(output.status.code(), Some(code), "{card}");
            assert_eq!(card["verdict"], "fail", "{card}");
            let gate = card["gates"]
                .as_array()
                .unwrap()
                .iter()
                .find(|gate| gate["id"] == "secrets")
                .unwrap();
            assert_eq!(gate["pass"], false);
            assert_eq!(gate["enforced"], true);
            assert_eq!(card["scores"]["security"], 0.75);
            let findings = card["findings"].as_array().unwrap();
            assert_eq!(findings.len(), 1, "{card}");
            assert_eq!(findings[0]["rule"], "secrets.empty_scope");
            assert_eq!(findings[0]["severity"], "error");
            assert!(findings[0]["message"]
                .as_str()
                .unwrap()
                .contains("No files"));
        }
        std::fs::write(root.join("analyzer.toml"), "pack = \"web\"\n").unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_sc"))
            .current_dir(&root)
            .args(["analyze", ".", "--format", "json", "--fail-on", "secrets"])
            .output()
            .unwrap();
        let card: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(output.status.code(), Some(0), "{card}");
        assert_eq!(card["scores"]["security"], 1.0);
        assert_eq!(card["findings"], serde_json::json!([]));
    }
    std::fs::remove_dir_all(root).unwrap();
}
