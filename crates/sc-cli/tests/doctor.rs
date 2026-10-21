// SPDX-License-Identifier: MIT
use std::process::Command;

fn project(name: &str, file: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("sc-doctor-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join(file), "").unwrap();
    root
}

#[test]
fn doctor_with_no_tools_on_path_lists_each_missing_tool_and_exits_1() {
    let root = project("rust", "Cargo.toml");
    let empty = root.join("empty-path");
    std::fs::create_dir_all(&empty).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_sc"))
        .args(["doctor"])
        .arg(&root)
        .env("PATH", &empty)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stdout}{stderr}");
    assert!(stdout.starts_with("rust pack\n"), "{stdout}");
    assert!(
        stdout.contains("MISSING   clippy (required, used for lint)\n            install: rustup component add clippy\n"),
        "{stdout}"
    );
    assert!(
        stdout.contains("install: rustup component add llvm-tools-preview && cargo install cargo-llvm-cov --locked"),
        "{stdout}"
    );
    assert!(stderr.contains("re-run `sc doctor`"), "{stderr}");
}

#[test]
fn doctor_on_a_pack_without_tools_exits_0() {
    let root = project("web", "index.html");
    let output = Command::new(env!("CARGO_BIN_EXE_sc"))
        .args(["doctor"])
        .arg(&root)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "web pack\n  no external tools needed\n"
    );
}

#[test]
fn doctor_names_the_fix_when_no_pack_matches() {
    let root = project("none", "notes.txt");
    let output = Command::new(env!("CARGO_BIN_EXE_sc"))
        .args(["doctor"])
        .arg(&root)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Pass --pack rust"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
