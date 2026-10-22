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

fn fake_tool(dir: &std::path::Path, name: &str) {
    let path = dir.join(name);
    std::fs::write(&path, "#!/bin/sh\nexit 1\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn doctor(root: &std::path::Path, path_dir: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_sc"))
        .args(["doctor"])
        .arg(root)
        .env("PATH", path_dir)
        .output()
        .unwrap()
}

#[test]
fn doctor_refuses_an_ambiguous_tree_like_analyze_does() {
    let root = project("ambiguous", "Cargo.toml");
    std::fs::write(root.join("package.json"), "{}").unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "").unwrap();
    std::fs::write(root.join("src/index.js"), "").unwrap();
    let output = doctor(&root, &root);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "several language packs match (rust, node); set pack in analyzer.toml or pass --pack"
        ),
        "{stderr}"
    );
    assert_eq!(output.stdout, b"");
}

#[test]
fn doctor_does_not_demand_gxx_for_a_c_only_tree() {
    let root = project("c-only", "main.c");
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    fake_tool(&bin, "cc");
    std::fs::write(root.join("Makefile"), "all:\n").unwrap();
    let output = doctor(&root, &bin);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{stdout}");
    assert!(stdout.contains("ok        cc\n"), "{stdout}");
    assert!(stdout.contains("missing   g++ (optional"), "{stdout}");
}

#[test]
fn doctor_does_not_demand_pytest_without_a_suite() {
    let root = project("py-nosuite", "requirements.txt");
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    fake_tool(&bin, "python3");
    fake_tool(&bin, "ruff");
    let output = doctor(&root, &bin);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{stdout}");
    assert!(
        stdout.contains("missing   pytest (optional, used for tests (no pytest suite found))"),
        "{stdout}"
    );
}
