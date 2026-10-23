// SPDX-License-Identifier: MIT
use std::process::Command;

fn analyze(root: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_sc"))
        .current_dir(root)
        .env("SCORECARD_NO_AGENT_SKILLS", "1")
        .args([
            "analyze",
            ".",
            "--format",
            "json",
            "--fail-on",
            "secrets",
            "--quiet",
        ])
        .output()
        .unwrap()
}

#[test]
fn macos_metadata_files_are_warned_about_and_not_scanned() {
    let root = std::env::temp_dir().join(format!("sc-os-metadata-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("analyzer.toml"), "pack = \"web\"\n").unwrap();
    std::fs::write(root.join("src/app.js"), "export const a = 1;\n").unwrap();

    let clean = analyze(&root);
    assert!(!String::from_utf8_lossy(&clean.stderr).contains("macOS metadata"));

    std::fs::write(root.join("src/._app.js"), [0u8, 5, 0x16, 7, 0, 2]).unwrap();
    std::fs::write(root.join(".DS_Store"), [0u8, 0, 0, 1]).unwrap();
    let output = analyze(&root);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ignoring 2 macOS metadata file(s) such as .DS_Store"),
        "{stderr}"
    );
    assert!(stderr.contains("-name '._*'"), "{stderr}");
    let card: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        !card.to_string().contains("._app.js"),
        "AppleDouble file reached the report: {card}"
    );
}

#[test]
fn secrets_in_metadata_named_files_are_found_with_and_without_git() {
    let parts = ["Ab", "3k", "Qm", "9Z", "nR", "4p", "Lx", "7w"];
    let tail: String = parts.iter().cycle().take(18).copied().collect();
    let root = std::env::temp_dir().join(format!("sc-os-metadata-secret-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("analyzer.toml"), "pack = \"web\"\n").unwrap();
    std::fs::write(root.join("app.js"), "export const a = 1;\n").unwrap();
    std::fs::write(
        root.join("._credential.txt"),
        format!("API_KEY=ghp_{tail}\n"),
    )
    .unwrap();

    let found = |output: &std::process::Output| {
        let card: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let hit = card["findings"].as_array().unwrap().iter().any(|finding| {
            finding["rule"] == "secrets.github_token" && finding["file"] == "._credential.txt"
        });
        (output.status.code(), hit)
    };
    assert_eq!(found(&analyze(&root)), (Some(1), true), "outside Git");

    let init = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&root)
        .status()
        .unwrap();
    assert!(init.success());
    assert_eq!(found(&analyze(&root)), (Some(1), true), "inside Git");
}
