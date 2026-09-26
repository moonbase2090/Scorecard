// SPDX-License-Identifier: MPL-2.0
//! `analyzer.toml` loading.
//!
//! Search order: `--config`, then `./analyzer.toml` in the project, then
//! `~/.config/sc/analyzer.toml`.

use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const KNOWN_GATES: &[&str] = &[
    "types", "tests", "crap", "secrets", "sca", "spec", "mutation", "lint",
];

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Config {
    pub gates: GatesConfig,
    pub scope: ScopeConfig,
    pub mutation: MutationConfig,
    pub llm: LlmConfig,
    pub engines: EnginesConfig,
    pub commands: CommandsConfig,
    /// Empty detects a pack from the tree: `rust`, `node`, `python`, `bash`, `go`, `java`, `csharp`, `php`, `cpp`, or `command`.
    pub pack: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct GatesConfig {
    pub fail_on: Vec<String>,
    pub crap_threshold: u32,
    pub new_fn_untested_cc: u32,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ScopeConfig {
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct MutationConfig {
    pub mode: String,
    pub max_mutants: u32,
    pub budget_seconds: u64,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LlmConfig {
    pub enabled: bool,
    pub endpoint: String,
    pub model: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct EnginesConfig {
    pub coverage: bool,
    pub sca: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct CommandsConfig {
    /// Empty skips the lint engine.
    pub lint: String,
}

impl Default for GatesConfig {
    fn default() -> Self {
        Self {
            fail_on: vec![
                "types".into(),
                "tests".into(),
                "crap".into(),
                "secrets".into(),
                "lint".into(),
            ],
            crap_threshold: 30,
            new_fn_untested_cc: 15,
        }
    }
}

impl Default for ScopeConfig {
    fn default() -> Self {
        Self {
            exclude: vec!["target/**".into(), "generated/**".into()],
        }
    }
}

impl Default for MutationConfig {
    fn default() -> Self {
        Self {
            mode: "off".into(),
            max_mutants: 50,
            budget_seconds: 180,
        }
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: "http://127.0.0.1:11434/v1".into(),
            model: "qwen2.5-coder".into(),
        }
    }
}

impl Default for CommandsConfig {
    fn default() -> Self {
        Self {
            lint: "cargo clippy -- -D warnings".into(),
        }
    }
}

impl Default for EnginesConfig {
    fn default() -> Self {
        Self {
            coverage: true,
            sca: true,
        }
    }
}

pub fn normalize_gates(list: &[String]) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for item in list {
        for part in item.split(',') {
            let gate = part.trim().to_ascii_lowercase();
            if gate.is_empty() {
                continue;
            }
            if !KNOWN_GATES.contains(&gate.as_str()) {
                return Err(format!("unknown gate `{gate}`"));
            }
            if !out.contains(&gate) {
                out.push(gate);
            }
        }
    }
    Ok(out)
}

pub fn resolve_config_path(explicit: Option<&Path>, project_root: &Path) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return Some(path.to_path_buf());
    }
    let local = project_root.join("analyzer.toml");
    if local.is_file() {
        return Some(local);
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home_cfg = PathBuf::from(home)
            .join(".config")
            .join("sc")
            .join("analyzer.toml");
        if home_cfg.is_file() {
            return Some(home_cfg);
        }
    }
    None
}

pub fn load_config_file(path: Option<&Path>) -> Result<Config, String> {
    let mut config = match path {
        None => Config::default(),
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|err| format!("read {}: {err}", path.display()))?;
            toml::from_str(&text).map_err(|err| format!("parse {}: {err}", path.display()))?
        }
    };
    config.gates.fail_on = normalize_gates(&config.gates.fail_on)?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_m1() {
        let config = Config::default();
        assert_eq!(
            config.gates.fail_on,
            vec!["types", "tests", "crap", "secrets", "lint"]
        );
        assert_eq!(config.gates.crap_threshold, 30);
        assert!(config.engines.coverage);
        assert!(config.engines.sca);
        assert!(config.pack.is_empty());
        assert_eq!(config.gates.new_fn_untested_cc, 15);
        assert!(!config.llm.enabled);
        assert_eq!(config.mutation.mode, "off");
    }

    #[test]
    fn example_file_parses() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../analyzer.toml.example");
        let config = load_config_file(Some(&path)).unwrap();
        assert_eq!(
            config.gates.fail_on,
            vec!["types", "tests", "crap", "secrets", "lint"]
        );
        assert_eq!(config.gates.crap_threshold, 30);
        assert_eq!(config.scope.exclude, vec!["target/**", "generated/**"]);
    }

    #[test]
    fn repo_dogfood_config_is_explicit() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../analyzer.toml");
        let config = load_config_file(Some(&path)).unwrap();
        assert_eq!(config.gates.crap_threshold, 420);
        assert_eq!(config.gates.new_fn_untested_cc, 21);
        assert!(config.gates.fail_on.iter().any(|gate| gate == "crap"));
    }

    #[test]
    fn rejects_unknown_gates() {
        let err = normalize_gates(&["types,nope".into()]).unwrap_err();
        assert!(err.contains("nope"));
    }
}
