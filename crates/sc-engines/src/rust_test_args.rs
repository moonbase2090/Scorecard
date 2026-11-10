// SPDX-License-Identifier: MIT
//! Argv for `cargo test` and `cargo llvm-cov`, including `commands.test_args`.

pub(crate) struct CargoInvocation {
    pub args: Vec<String>,
    pub display: String,
}

pub(crate) fn test_invocation(
    workspace_root: bool,
    extra: &[String],
    test_name: Option<&str>,
    manifest: &str,
) -> CargoInvocation {
    let (cargo_args, harness) = split_test_args(extra);
    let mut args = vec!["test".to_string()];
    if workspace_root {
        args.push("--workspace".to_string());
    }
    args.extend(cargo_args.iter().cloned());
    if let Some(name) = test_name {
        args.push(name.to_string());
    }
    let shown = args.len();
    args.extend([
        "--manifest-path".to_string(),
        manifest.to_string(),
        "--color".to_string(),
        "never".to_string(),
    ]);
    append_harness(&mut args, harness);
    CargoInvocation {
        display: display_command(&args, shown, harness),
        args,
    }
}

pub(crate) fn coverage_invocation(
    workspace_root: bool,
    extra: &[String],
    manifest: &str,
    output_path: &str,
) -> CargoInvocation {
    let (cargo_args, harness) = split_test_args(extra);
    let mut args = vec!["llvm-cov".to_string()];
    if workspace_root {
        args.push("--workspace".to_string());
    }
    args.extend(cargo_args.iter().cloned());
    args.push("--json".to_string());
    let shown = args.len();
    args.extend([
        "--output-path".to_string(),
        output_path.to_string(),
        "--manifest-path".to_string(),
        manifest.to_string(),
        "--no-clean".to_string(),
    ]);
    append_harness(&mut args, harness);
    CargoInvocation {
        display: display_command(&args, shown, harness),
        args,
    }
}

fn split_test_args(extra: &[String]) -> (&[String], &[String]) {
    match extra.iter().position(|arg| arg == "--") {
        Some(index) => (&extra[..index], &extra[index + 1..]),
        None => (extra, &[]),
    }
}

fn append_harness(args: &mut Vec<String>, harness: &[String]) {
    if harness.is_empty() {
        return;
    }
    args.push("--".to_string());
    args.extend(harness.iter().cloned());
}

fn display_command(args: &[String], shown: usize, harness: &[String]) -> String {
    let mut parts = Vec::with_capacity(shown + harness.len() + 2);
    parts.push("cargo".to_string());
    parts.extend(args.iter().take(shown).cloned());
    if !harness.is_empty() {
        parts.push("--".to_string());
        parts.extend(harness.iter().cloned());
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_test_command_matches_the_historical_invocation() {
        let invocation = test_invocation(false, &[], None, "/proj/Cargo.toml");
        assert_eq!(invocation.display, "cargo test");
        assert_eq!(
            invocation.args,
            vec![
                "test",
                "--manifest-path",
                "/proj/Cargo.toml",
                "--color",
                "never",
            ]
        );
        let workspace = test_invocation(true, &[], None, "/proj/Cargo.toml");
        assert_eq!(workspace.display, "cargo test --workspace");
        assert_eq!(workspace.args[1], "--workspace");
    }

    #[test]
    fn extra_args_reach_cargo_and_the_harness_in_order() {
        let extra = vec![
            "--locked".into(),
            "--features".into(),
            "extra".into(),
            "--".into(),
            "extra_on".into(),
            "--test-threads=1".into(),
        ];
        let invocation = test_invocation(false, &extra, None, "/proj/Cargo.toml");
        assert_eq!(
            invocation.display,
            "cargo test --locked --features extra -- extra_on --test-threads=1"
        );
        assert_eq!(
            invocation.args,
            vec![
                "test",
                "--locked",
                "--features",
                "extra",
                "--manifest-path",
                "/proj/Cargo.toml",
                "--color",
                "never",
                "--",
                "extra_on",
                "--test-threads=1",
            ]
        );
    }

    #[test]
    fn a_configured_filter_and_a_diff_selected_name_are_both_filters() {
        let extra = vec!["--".into(), "extra_on".into()];
        let invocation = test_invocation(false, &extra, Some("mux::keeps_time"), "/p/Cargo.toml");
        assert_eq!(invocation.display, "cargo test mux::keeps_time -- extra_on");
        assert_eq!(
            invocation.args,
            vec![
                "test",
                "mux::keeps_time",
                "--manifest-path",
                "/p/Cargo.toml",
                "--color",
                "never",
                "--",
                "extra_on",
            ]
        );
    }

    #[test]
    fn coverage_passes_a_configured_filter_after_the_separator() {
        let extra = vec!["--".into(), "extra_on".into()];
        let invocation = coverage_invocation(false, &extra, "/p/Cargo.toml", "/p/out.json");
        assert_eq!(invocation.args[0], "llvm-cov");
        assert_eq!(invocation.args[1], "--json");
        assert_eq!(
            &invocation.args[invocation.args.len() - 2..],
            ["--", "extra_on"]
        );
        assert!(!invocation.args[1..invocation.args.len() - 2].contains(&"extra_on".to_string()));
    }

    #[test]
    fn named_diff_test_keeps_cargo_options_and_the_harness_filter() {
        let extra = vec!["--locked".into(), "--".into(), "--test-threads=1".into()];
        let invocation = test_invocation(true, &extra, Some("mux::keeps_time"), "/ws/Cargo.toml");
        assert_eq!(
            invocation.display,
            "cargo test --workspace --locked mux::keeps_time -- --test-threads=1"
        );
        assert_eq!(
            invocation.args,
            vec![
                "test",
                "--workspace",
                "--locked",
                "mux::keeps_time",
                "--manifest-path",
                "/ws/Cargo.toml",
                "--color",
                "never",
                "--",
                "--test-threads=1",
            ]
        );
    }

    #[test]
    fn coverage_uses_the_same_extra_args_without_changing_the_default() {
        let plain = coverage_invocation(true, &[], "/proj/Cargo.toml", "/proj/out.json");
        assert_eq!(plain.display, "cargo llvm-cov --workspace --json");
        assert_eq!(
            plain.args,
            vec![
                "llvm-cov",
                "--workspace",
                "--json",
                "--output-path",
                "/proj/out.json",
                "--manifest-path",
                "/proj/Cargo.toml",
                "--no-clean",
            ]
        );
        let member = coverage_invocation(false, &[], "/m/Cargo.toml", "/m/out.json");
        assert_eq!(member.display, "cargo llvm-cov --json");
        let extra = vec![
            "--features".into(),
            "extra".into(),
            "--".into(),
            "--test-threads=1".into(),
        ];
        let with_args = coverage_invocation(true, &extra, "/proj/Cargo.toml", "/proj/out.json");
        assert_eq!(
            with_args.display,
            "cargo llvm-cov --workspace --features extra --json -- --test-threads=1"
        );
        assert_eq!(
            with_args.args,
            vec![
                "llvm-cov",
                "--workspace",
                "--features",
                "extra",
                "--json",
                "--output-path",
                "/proj/out.json",
                "--manifest-path",
                "/proj/Cargo.toml",
                "--no-clean",
                "--",
                "--test-threads=1",
            ]
        );
    }
}
