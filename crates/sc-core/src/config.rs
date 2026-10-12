// SPDX-License-Identifier: MPL-2.0
//! `analyzer.toml` loading.
//!
//! Search order: `--config`, then `./analyzer.toml` in the project, then
//! `~/.config/sc/analyzer.toml`.

use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const KNOWN_GATES: &[&str] = &[
    "types", "tests", "crap", "secrets", "sca", "spec", "mutation", "lint", "html", "links", "a11y",
];

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Config {
    /// Install or update the Scorecard agent skill during automatic install hooks.
    pub install_agent_skills: bool,
    pub gates: GatesConfig,
    pub scope: ScopeConfig,
    pub mutation: MutationConfig,
    pub llm: LlmConfig,
    pub engines: EnginesConfig,
    pub commands: CommandsConfig,
    pub html: HtmlConfig,
    pub links: LinksConfig,
    pub a11y: A11yConfig,
    /// Empty detects a pack from the tree: `rust`, `node`, `python`, `bash`, `go`, `java`, `csharp`, `php`, `cpp`, `web`, or `command`.
    pub pack: String,
    /// Rustup channel for check, test, coverage, and lint. Empty uses
    /// `rust-toolchain.toml` / `rust-toolchain` in the project root.
    pub toolchain: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            install_agent_skills: true,
            gates: GatesConfig::default(),
            scope: ScopeConfig::default(),
            mutation: MutationConfig::default(),
            llm: LlmConfig::default(),
            engines: EnginesConfig::default(),
            commands: CommandsConfig::default(),
            html: HtmlConfig::default(),
            links: LinksConfig::default(),
            a11y: A11yConfig::default(),
            pack: String::new(),
            toolchain: String::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct GatesConfig {
    pub fail_on: Vec<String>,
    pub crap_threshold: u32,
    pub new_fn_untested_cc: u32,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ScopeConfig {
    pub exclude: Vec<String>,
    /// Override built-in skips for generated or vendored directories and
    /// generated source markers. `.gitignore` and `exclude` still apply.
    pub include_generated: Vec<String>,
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
    /// `ollama` (default), `cursor`, or `openai-compatible`.
    /// `cursor` sends the spec and the files the agent reads to Cursor.
    /// `openai-compatible` sends the spec and tool-read file text to `base_url`.
    pub backend: String,
    /// Used when `backend` is `openai-compatible`.
    pub base_url: String,
    /// Name of the environment variable that holds the API key. The key
    /// itself is never stored here.
    pub api_key_env: String,
    /// Tool-using turns before the model must return a spec-gap verdict.
    pub max_tool_rounds: u32,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct EnginesConfig {
    pub coverage: bool,
    pub sca: bool,
    /// When true, emit `perf.nested_loop` / `perf.clone_in_loop` on product
    /// Rust (test modules stay skipped). Off by default.
    pub perf: bool,
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
            backend: "ollama".into(),
            base_url: "https://openrouter.ai/api/v1".into(),
            api_key_env: "OPENROUTER_API_KEY".into(),
            max_tool_rounds: 36,
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct HtmlConfig {
    /// `auto` (default), `on`, or `off`.
    ///
    /// `auto` enforces the html gate when `fail_on` is the built-in list or
    /// already names `html`. A custom list enforces it only when it names `html`.
    pub enforce: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LinksConfig {
    /// When true, a missing internal file fails the process.
    pub enforce: bool,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct A11yConfig {
    /// When true, accessibility findings fail the process.
    pub enforce: bool,
    /// Rule ids to skip, such as `img-alt` or `contrast`.
    pub disable: Vec<String>,
}

impl Default for HtmlConfig {
    fn default() -> Self {
        Self {
            enforce: "auto".into(),
        }
    }
}

impl Default for CommandsConfig {
    fn default() -> Self {
        Self {
            lint: "cargo clippy --workspace".into(),
        }
    }
}

impl Default for EnginesConfig {
    fn default() -> Self {
        Self {
            coverage: true,
            sca: true,
            perf: false,
        }
    }
}

pub fn normalize_gates(list: &[String]) -> Result<Vec<String>, String> {
    let parts: Vec<_> = list.iter().flat_map(|item| item.split(',')).collect();
    if parts
        .iter()
        .any(|part| part.trim().eq_ignore_ascii_case("none"))
    {
        if parts.len() == 1 && parts[0].trim().eq_ignore_ascii_case("none") {
            return Ok(Vec::new());
        }
        return Err(
            "`none` must be the only fail-on value. Pass --fail-on none by itself or set gates.fail_on = [\"none\"].".into(),
        );
    }

    let mut out = Vec::new();
    for part in parts {
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
    if out.is_empty() {
        return Err(
            "No gate can fail this run because the fail-on selection is empty. Omit the --fail-on option or the gates.fail_on setting to use the default gates, name one or more gates, or pass --fail-on none for report-only output.".into(),
        );
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
    if let Some(home_cfg) = user_config_path() {
        if home_cfg.is_file() {
            return Some(home_cfg);
        }
    }
    None
}

pub fn user_config_file(home: &Path) -> PathBuf {
    home.join(".config").join("sc").join("analyzer.toml")
}

pub fn user_config_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| user_config_file(Path::new(&home)))
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

    #[test]
    fn toolchain_from_analyzer_toml() {
        let config: Config = toml::from_str("toolchain = \"1.85.0\"\n").unwrap();
        assert_eq!(config.toolchain, "1.85.0");
        let empty: Config = toml::from_str("").unwrap();
        assert_eq!(empty.toolchain, "");
    }
    use super::*;

    #[test]
    fn defaults_match_m1() {
        let config = Config::default();
        assert!(config.install_agent_skills);
        assert_eq!(
            config.gates.fail_on,
            vec!["types", "tests", "crap", "secrets", "lint"]
        );
        assert_eq!(config.gates.crap_threshold, 30);
        assert!(config.engines.coverage);
        assert!(config.engines.sca);
        assert!(!config.engines.perf);
        assert!(config.pack.is_empty());
        assert_eq!(config.gates.new_fn_untested_cc, 15);
        assert!(!config.llm.enabled);
        assert_eq!(config.mutation.mode, "off");
        assert_eq!(config.commands.lint, "cargo clippy --workspace");
        assert!(!config.commands.lint.contains("-D warnings"));
    }

    #[test]
    fn agent_skill_installation_can_be_disabled_in_config() {
        let config: Config = toml::from_str("install_agent_skills = false\n").unwrap();
        assert!(!config.install_agent_skills);
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
        assert!(config.scope.exclude.is_empty());
        assert!(config.scope.include_generated.is_empty());
    }

    #[test]
    fn repo_dogfood_config_is_explicit() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../analyzer.toml");
        let config = load_config_file(Some(&path)).unwrap();
        assert_eq!(config.gates.crap_threshold, 30);
        assert_eq!(config.gates.new_fn_untested_cc, 15);
        assert!(config.gates.fail_on.iter().any(|gate| gate == "crap"));
        assert_eq!(config.pack, "rust");
    }

    #[test]
    fn rejects_unknown_gates() {
        let err = normalize_gates(&["types,nope".into()]).unwrap_err();
        assert!(err.contains("nope"));
    }

    #[test]
    fn rejects_empty_gate_lists() {
        assert!(normalize_gates(&["".into()]).is_err());
        assert!(normalize_gates(&[" , ".into()]).is_err());
    }

    #[test]
    fn none_must_be_the_only_gate_value() {
        for value in ["none,", ",none", "none,lint"] {
            let err = normalize_gates(&[value.into()]).unwrap_err();
            assert!(err.contains("`none` must be the only"), "{err}");
        }
        assert!(normalize_gates(&[" none ".into()]).unwrap().is_empty());
    }
}
