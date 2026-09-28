// SPDX-License-Identifier: MPL-2.0
//! Resolve the Rust toolchain pin for check, test, coverage, and lint.
//!
//! `analyzer.toml` `toolchain` wins. Otherwise the channel from
//! `rust-toolchain.toml` or `rust-toolchain` in the project root is used.
//! An inherited `RUSTUP_TOOLCHAIN` is cleared when neither names a channel,
//! so a parent CI pin cannot hide the project's file.

use std::path::Path;
use std::process::Command;

/// Prefer a non-empty `analyzer.toml` `toolchain`, else the project file pin.
pub fn resolve(root: &Path, config_pin: &str) -> Option<String> {
    let trimmed = config_pin.trim();
    if !trimmed.is_empty() {
        return Some(trimmed.to_string());
    }
    read_file_pin(root)
}

/// Set `RUSTUP_TOOLCHAIN` when a pin is known; otherwise remove it.
pub fn apply(cmd: &mut Command, pin: Option<&str>) {
    match pin {
        Some(channel) if !channel.is_empty() => {
            cmd.env("RUSTUP_TOOLCHAIN", channel);
        }
        _ => {
            cmd.env_remove("RUSTUP_TOOLCHAIN");
        }
    }
}

fn read_file_pin(root: &Path) -> Option<String> {
    let toml_path = root.join("rust-toolchain.toml");
    if toml_path.is_file() {
        return parse_toolchain_toml(&std::fs::read_to_string(toml_path).ok()?);
    }
    let legacy = root.join("rust-toolchain");
    if legacy.is_file() {
        return parse_toolchain_text(&std::fs::read_to_string(legacy).ok()?);
    }
    None
}

fn parse_toolchain_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with('[') {
        return parse_toolchain_toml(trimmed);
    }
    let channel = trimmed.lines().next()?.trim();
    if channel.is_empty() || channel.starts_with('#') {
        None
    } else {
        Some(channel.to_string())
    }
}

fn parse_toolchain_toml(text: &str) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct File {
        toolchain: Option<Toolchain>,
    }
    #[derive(serde::Deserialize)]
    struct Toolchain {
        channel: Option<String>,
    }
    let file: File = toml::from_str(text).ok()?;
    let channel = file.toolchain?.channel?;
    let channel = channel.trim();
    if channel.is_empty() {
        None
    } else {
        Some(channel.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;
    use std::time::Duration;

    #[test]
    fn config_pin_wins_over_rust_toolchain_toml() {
        let root = std::env::temp_dir().join(format!("sc-toolchain-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"stable\"\n",
        )
        .unwrap();
        assert_eq!(resolve(&root, "1.85.0").as_deref(), Some("1.85.0"));
        assert_eq!(resolve(&root, "").as_deref(), Some("stable"));
        assert_eq!(resolve(&root, "  ").as_deref(), Some("stable"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn legacy_rust_toolchain_file_names_the_channel() {
        let root = std::env::temp_dir().join(format!("sc-toolchain-legacy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("rust-toolchain"), "nightly-2024-01-01\n").unwrap();
        assert_eq!(resolve(&root, "").as_deref(), Some("nightly-2024-01-01"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_sets_or_clears_rustup_toolchain() {
        let mut set = Command::new("sh");
        set.arg("-c")
            .arg("printf %s \"$RUSTUP_TOOLCHAIN\"")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("RUSTUP_TOOLCHAIN", "should-be-replaced");
        apply(&mut set, Some("1.85.0"));
        let captured = crate::command::run_cmd(&mut set, Duration::from_secs(5)).unwrap();
        assert!(captured.status.success());
        assert_eq!(captured.stdout, "1.85.0");

        let mut clear = Command::new("sh");
        clear
            .arg("-c")
            .arg("printf %s \"$RUSTUP_TOOLCHAIN\"")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("RUSTUP_TOOLCHAIN", "inherited");
        apply(&mut clear, None);
        let captured = crate::command::run_cmd(&mut clear, Duration::from_secs(5)).unwrap();
        assert!(captured.status.success());
        assert_eq!(captured.stdout, "");
    }
}
