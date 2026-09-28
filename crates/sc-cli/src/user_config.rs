// SPDX-License-Identifier: MPL-2.0
//! User-level `~/.config/sc/analyzer.toml`.

use std::fs;
use std::path::Path;

const STARTER: &str = include_str!("../../../analyzer.toml.example");

pub enum InitResult {
    Wrote,
    Kept,
}

pub fn init_config(path: &Path, force: bool) -> Result<InitResult, String> {
    if path.is_file() && !force {
        return Ok(InitResult::Kept);
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "could not create {}: {err}. Create that directory and re-run `sc config init`.",
                    parent.display()
                )
            })?;
        }
    }
    fs::write(path, STARTER).map_err(|err| {
        format!(
            "could not write {}: {err}. Check that the directory is writable and re-run `sc config init`.",
            path.display()
        )
    })?;
    Ok(InitResult::Wrote)
}

pub fn run_path() -> i32 {
    match sc_core::user_config_path() {
        Some(path) => {
            println!("{}", path.display());
            0
        }
        None => {
            eprintln!(
                "HOME is not set, so ~/.config/sc/analyzer.toml cannot be resolved. Set HOME and re-run `sc config path`."
            );
            1
        }
    }
}

pub fn run_init(force: bool) -> i32 {
    let Some(path) = sc_core::user_config_path() else {
        eprintln!(
            "HOME is not set, so ~/.config/sc/analyzer.toml cannot be created. Set HOME and re-run `sc config init`."
        );
        return 1;
    };
    match init_config(&path, force) {
        Ok(InitResult::Wrote) => {
            println!("Wrote {}", path.display());
            0
        }
        Ok(InitResult::Kept) => {
            println!(
                "{} already exists and was left unchanged. Run `sc config init --force` to replace it.",
                path.display()
            );
            0
        }
        Err(err) => {
            eprintln!("{err}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_creates_the_starter_and_does_not_overwrite() {
        let dir = std::env::temp_dir().join(format!("sc-cfg-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = sc_core::user_config_file(&dir);
        assert!(matches!(
            init_config(&path, false).unwrap(),
            InitResult::Wrote
        ));
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text, STARTER);
        assert!(text.contains("# Scorecard configuration."));
        fs::write(&path, "keep\n").unwrap();
        assert!(matches!(
            init_config(&path, false).unwrap(),
            InitResult::Kept
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), "keep\n");
        assert!(matches!(
            init_config(&path, true).unwrap(),
            InitResult::Wrote
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), STARTER);
        let _ = fs::remove_dir_all(&dir);
    }
}
