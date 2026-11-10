// SPDX-License-Identifier: MIT
//! Extra `cargo test` and `cargo llvm-cov` arguments from `[rust] test_args`.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SplitTestArgs {
    pub cargo: Vec<String>,
    pub harness: Vec<String>,
}

pub(crate) fn split_test_args(test_args: &[String]) -> SplitTestArgs {
    match test_args.iter().position(|arg| arg == "--") {
        Some(index) => SplitTestArgs {
            cargo: test_args[..index].to_vec(),
            harness: test_args[index + 1..].to_vec(),
        },
        None => SplitTestArgs {
            cargo: test_args.to_vec(),
            harness: Vec::new(),
        },
    }
}

/// `cargo test` argv. `sc` still supplies `--workspace` (at a workspace root),
/// `--manifest-path`, and `--color never`. Caller arguments stay around those.
pub(crate) fn cargo_test_argv(
    workspace_root: bool,
    manifest: &str,
    filter: Option<&str>,
    test_args: &[String],
) -> Vec<String> {
    let split = split_test_args(test_args);
    let mut args = Vec::new();
    args.push("test".to_string());
    if workspace_root {
        args.push("--workspace".to_string());
    }
    args.extend(split.cargo);
    args.push("--manifest-path".to_string());
    args.push(manifest.to_string());
    args.push("--color".to_string());
    args.push("never".to_string());
    if let Some(filter) = filter {
        args.push(filter.to_string());
    }
    if !split.harness.is_empty() {
        args.push("--".to_string());
        args.extend(split.harness);
    }
    args
}

pub(crate) fn cargo_llvm_cov_argv(
    workspace_root: bool,
    manifest: &str,
    output: &str,
    test_args: &[String],
) -> Vec<String> {
    let split = split_test_args(test_args);
    let mut args = Vec::new();
    args.push("llvm-cov".to_string());
    if workspace_root {
        args.push("--workspace".to_string());
    }
    args.extend(split.cargo);
    args.push("--json".to_string());
    args.push("--output-path".to_string());
    args.push(output.to_string());
    args.push("--manifest-path".to_string());
    args.push(manifest.to_string());
    args.push("--no-clean".to_string());
    if !split.harness.is_empty() {
        args.push("--".to_string());
        args.extend(split.harness);
    }
    args
}

/// Command shown in the report. Omits the manifest path and color flag `sc` adds.
pub(crate) fn test_command_display(
    workspace_root: bool,
    filter: Option<&str>,
    test_args: &[String],
) -> String {
    let split = split_test_args(test_args);
    let mut parts = vec!["cargo".to_string(), "test".to_string()];
    if workspace_root {
        parts.push("--workspace".to_string());
    }
    parts.extend(split.cargo);
    if let Some(filter) = filter {
        parts.push(filter.to_string());
    }
    if !split.harness.is_empty() {
        parts.push("--".to_string());
        parts.extend(split.harness);
    }
    parts.join(" ")
}

pub(crate) fn coverage_command_display(workspace_root: bool, test_args: &[String]) -> String {
    let split = split_test_args(test_args);
    let mut parts = vec!["cargo".to_string(), "llvm-cov".to_string()];
    if workspace_root {
        parts.push("--workspace".to_string());
    }
    parts.extend(split.cargo);
    parts.push("--json".to_string());
    if !split.harness.is_empty() {
        parts.push("--".to_string());
        parts.extend(split.harness);
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_commands_match_the_historical_cargo_lines() {
        assert_eq!(
            cargo_test_argv(true, "/repo/Cargo.toml", None, &[]),
            vec![
                "test",
                "--workspace",
                "--manifest-path",
                "/repo/Cargo.toml",
                "--color",
                "never",
            ]
        );
        assert_eq!(
            cargo_test_argv(false, "Cargo.toml", Some("passes"), &[]),
            vec![
                "test",
                "--manifest-path",
                "Cargo.toml",
                "--color",
                "never",
                "passes",
            ]
        );
        assert_eq!(
            cargo_llvm_cov_argv(true, "Cargo.toml", "out.json", &[]),
            vec![
                "llvm-cov",
                "--workspace",
                "--json",
                "--output-path",
                "out.json",
                "--manifest-path",
                "Cargo.toml",
                "--no-clean",
            ]
        );
        assert_eq!(
            test_command_display(true, None, &[]),
            "cargo test --workspace"
        );
        assert_eq!(test_command_display(false, None, &[]), "cargo test");
        assert_eq!(
            test_command_display(true, Some("passes"), &[]),
            "cargo test --workspace passes"
        );
        assert_eq!(
            coverage_command_display(true, &[]),
            "cargo llvm-cov --workspace --json"
        );
        assert_eq!(
            coverage_command_display(false, &[]),
            "cargo llvm-cov --json"
        );
    }

    #[test]
    fn features_and_harness_arguments_are_placed_around_sc_flags() {
        let extra = vec![
            "--locked".to_string(),
            "--features".to_string(),
            "extra".to_string(),
            "--".to_string(),
            "--test-threads=1".to_string(),
        ];
        assert_eq!(
            cargo_test_argv(true, "Cargo.toml", None, &extra),
            vec![
                "test",
                "--workspace",
                "--locked",
                "--features",
                "extra",
                "--manifest-path",
                "Cargo.toml",
                "--color",
                "never",
                "--",
                "--test-threads=1",
            ]
        );
        assert_eq!(
            cargo_test_argv(false, "Cargo.toml", Some("tests::passes"), &extra),
            vec![
                "test",
                "--locked",
                "--features",
                "extra",
                "--manifest-path",
                "Cargo.toml",
                "--color",
                "never",
                "tests::passes",
                "--",
                "--test-threads=1",
            ]
        );
        assert_eq!(
            cargo_llvm_cov_argv(true, "Cargo.toml", "out.json", &extra),
            vec![
                "llvm-cov",
                "--workspace",
                "--locked",
                "--features",
                "extra",
                "--json",
                "--output-path",
                "out.json",
                "--manifest-path",
                "Cargo.toml",
                "--no-clean",
                "--",
                "--test-threads=1",
            ]
        );
        assert_eq!(
            test_command_display(true, None, &extra),
            "cargo test --workspace --locked --features extra -- --test-threads=1"
        );
        assert_eq!(
            coverage_command_display(false, &extra),
            "cargo llvm-cov --locked --features extra --json -- --test-threads=1"
        );
    }
}
