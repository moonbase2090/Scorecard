// SPDX-License-Identifier: MPL-2.0
//! Choose how Cargo commands pick a rustup toolchain.
//!
//! - `analyzer.toml` `toolchain` sets `RUSTUP_TOOLCHAIN` to that channel.
//! - A `rust-toolchain.toml` / `rust-toolchain` at or above the project
//!   clears an inherited `RUSTUP_TOOLCHAIN` so rustup reads the file
//!   (channel, components, targets, profile).
//! - Otherwise the inherited environment is left alone.

use std::path::{Path, PathBuf};
use std::process::Command;

/// How to treat `RUSTUP_TOOLCHAIN` for a Cargo (or lint) command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Policy {
    /// Set `RUSTUP_TOOLCHAIN` to this channel (`analyzer.toml`).
    ConfigPin(String),
    /// Clear `RUSTUP_TOOLCHAIN` so rustup honors a toolchain file.
    FilePin,
    /// Do not change `RUSTUP_TOOLCHAIN`.
    Inherit,
}

/// Prefer a non-empty `analyzer.toml` `toolchain`, else a toolchain file
/// at or above `root`, else leave the environment alone.
pub fn resolve(root: &Path, config_pin: &str) -> Policy {
    let trimmed = config_pin.trim();
    if !trimmed.is_empty() {
        return Policy::ConfigPin(trimmed.to_string());
    }
    if toolchain_file_at_or_above(root).is_some() {
        Policy::FilePin
    } else {
        Policy::Inherit
    }
}

/// Apply the policy to a command that will run Cargo or a lint shell.
pub fn apply(cmd: &mut Command, policy: &Policy) {
    match policy {
        Policy::ConfigPin(channel) => {
            cmd.env("RUSTUP_TOOLCHAIN", channel);
        }
        Policy::FilePin => {
            cmd.env_remove("RUSTUP_TOOLCHAIN");
        }
        Policy::Inherit => {}
    }
}

/// Channel string for the Docker fallback script only. `FilePin` and
/// `Inherit` leave the image default alone.
pub fn docker_env_prefix(policy: &Policy) -> Option<&str> {
    match policy {
        Policy::ConfigPin(channel) => Some(channel.as_str()),
        Policy::FilePin | Policy::Inherit => None,
    }
}

fn toolchain_file_at_or_above(root: &Path) -> Option<PathBuf> {
    let mut dir = root.to_path_buf();
    loop {
        let toml = dir.join("rust-toolchain.toml");
        if toml.is_file() {
            return Some(toml);
        }
        let legacy = dir.join("rust-toolchain");
        if legacy.is_file() {
            return Some(legacy);
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;
    use std::time::Duration;

    #[test]
    fn config_pin_wins_over_a_toolchain_file() {
        let root = std::env::temp_dir().join(format!("sc-toolchain-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"stable\"\n",
        )
        .unwrap();
        assert_eq!(resolve(&root, "1.85.0"), Policy::ConfigPin("1.85.0".into()));
        assert_eq!(resolve(&root, ""), Policy::FilePin);
        assert_eq!(resolve(&root, "  "), Policy::FilePin);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_parent_toolchain_file_is_a_file_pin() {
        let base = std::env::temp_dir().join(format!("sc-toolchain-parent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let child = base.join("member");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::write(
            base.join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"1.85\"\n",
        )
        .unwrap();
        assert_eq!(resolve(&child, ""), Policy::FilePin);
        assert_eq!(
            resolve(&child, "stable"),
            Policy::ConfigPin("stable".into())
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn legacy_rust_toolchain_file_is_a_file_pin() {
        let root = std::env::temp_dir().join(format!("sc-toolchain-legacy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("rust-toolchain"), "nightly-2024-01-01\n").unwrap();
        assert_eq!(resolve(&root, ""), Policy::FilePin);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn no_pin_leaves_inherited_rustup_toolchain() {
        assert_eq!(
            resolve(
                &std::env::temp_dir().join(format!("sc-toolchain-none-{}", std::process::id())),
                ""
            ),
            Policy::Inherit
        );

        let mut inherit = Command::new("sh");
        inherit
            .arg("-c")
            .arg("printf %s \"$RUSTUP_TOOLCHAIN\"")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("RUSTUP_TOOLCHAIN", "inherited");
        apply(&mut inherit, &Policy::Inherit);
        let captured = crate::command::run_cmd(&mut inherit, Duration::from_secs(5)).unwrap();
        assert!(captured.status.success());
        assert_eq!(captured.stdout, "inherited");

        let mut clear = Command::new("sh");
        clear
            .arg("-c")
            .arg("printf %s \"$RUSTUP_TOOLCHAIN\"")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("RUSTUP_TOOLCHAIN", "inherited");
        apply(&mut clear, &Policy::FilePin);
        let captured = crate::command::run_cmd(&mut clear, Duration::from_secs(5)).unwrap();
        assert!(captured.status.success());
        assert_eq!(captured.stdout, "");

        let mut set = Command::new("sh");
        set.arg("-c")
            .arg("printf %s \"$RUSTUP_TOOLCHAIN\"")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("RUSTUP_TOOLCHAIN", "should-be-replaced");
        apply(&mut set, &Policy::ConfigPin("1.85.0".into()));
        let captured = crate::command::run_cmd(&mut set, Duration::from_secs(5)).unwrap();
        assert!(captured.status.success());
        assert_eq!(captured.stdout, "1.85.0");
    }
}
