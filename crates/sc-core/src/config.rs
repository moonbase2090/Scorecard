// SPDX-License-Identifier: MIT
//! `analyzer.toml` loading.
//!
//! Search order: `--config`, then `./analyzer.toml` in the project, then
//! `~/.config/sc/analyzer.toml` (`%APPDATA%\sc\analyzer.toml` on Windows).

use std::ffi::OsString;
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
    /// Empty detects a pack from the tree; `multi` opts into sequential monorepo analysis.
    pub pack: String,
    /// Rustup channel for check, test, coverage, and lint. Empty uses
    /// `rust-toolchain.toml` / `rust-toolchain` in the project root.
    pub toolchain: String,
    /// Wall-clock seconds for pack commands. `sc analyze --budget-seconds`
    /// overrides this. Not `mutation.budget_seconds`, which caps mutation only.
    #[serde(default = "default_budget_seconds")]
    pub budget_seconds: u64,
    #[serde(default)]
    pub rust: RustConfig,
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
            budget_seconds: DEFAULT_BUDGET_SECONDS,
            rust: RustConfig::default(),
        }
    }
}

/// Default analysis clock, in seconds. Matches `sc analyze --budget-seconds`.
pub const DEFAULT_BUDGET_SECONDS: u64 = 120;

fn default_budget_seconds() -> u64 {
    DEFAULT_BUDGET_SECONDS
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct RustConfig {
    /// Extra arguments for `cargo test` and `cargo llvm-cov`.
    /// Cargo options come before `--`; later tokens go to the test harness.
    pub test_args: Vec<String>,
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
    /// Run the read-only Rust SCA and secrets scans alongside Cargo phases.
    /// Off by default so analysis scheduling remains opt-in.
    pub parallel: bool,
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
            parallel: false,
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

/// Shared text for the "no profile directory" error.
pub const PROFILE_UNSET: &str = if cfg!(windows) {
    "Neither HOME nor USERPROFILE is set."
} else {
    "HOME is not set."
};

/// The user's profile directory: `HOME`, then `USERPROFILE` on Windows.
pub fn user_profile_dir() -> Option<PathBuf> {
    profile_dir(
        std::env::var_os("HOME"),
        std::env::var_os("USERPROFILE"),
        cfg!(windows),
    )
}

fn profile_dir(
    home: Option<OsString>,
    userprofile: Option<OsString>,
    windows: bool,
) -> Option<PathBuf> {
    let set = |value: Option<OsString>| value.filter(|value| !value.is_empty());
    set(home)
        .or_else(|| set(userprofile).filter(|_| windows))
        .map(PathBuf::from)
}

/// Directory for `analyzer.toml` and the agent skill version stamp:
/// `<profile>/.config/sc`, or `%APPDATA%\sc` on Windows.
///
/// `APPDATA` belongs to the signed-in user, so it applies only when `profile`
/// is that user's profile. Any other profile (a test home, say) gets the
/// `<profile>\AppData\Roaming` default and never touches the real folder.
pub fn user_config_dir(profile: &Path) -> PathBuf {
    let appdata = appdata_for_profile(
        profile,
        std::env::var_os("APPDATA"),
        std::env::var_os("USERPROFILE"),
    );
    config_dir(profile, appdata.as_deref(), cfg!(windows))
}

/// `APPDATA` is the signed-in user's folder, so it applies only when `profile`
/// is that user's `USERPROFILE`. A `HOME` override (a test home, say) keeps the
/// `<profile>\AppData\Roaming` default.
fn appdata_for_profile(
    profile: &Path,
    appdata: Option<OsString>,
    userprofile: Option<OsString>,
) -> Option<PathBuf> {
    let native = userprofile.filter(|value| !value.is_empty())?;
    if Path::new(&native) != profile {
        return None;
    }
    appdata.filter(|value| !value.is_empty()).map(PathBuf::from)
}

fn config_dir(profile: &Path, appdata: Option<&Path>, windows: bool) -> PathBuf {
    if !windows {
        return profile.join(".config").join("sc");
    }
    appdata
        .filter(|path| !path.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| profile.join("AppData").join("Roaming"))
        .join("sc")
}

pub fn user_config_file(profile: &Path) -> PathBuf {
    user_config_dir(profile).join("analyzer.toml")
}

pub fn user_config_path() -> Option<PathBuf> {
    user_profile_dir().map(|profile| user_config_file(&profile))
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
    validate_budget_seconds(config.budget_seconds)?;
    validate_rust_test_args(&config.rust.test_args)?;
    Ok(config)
}

pub fn resolve_budget_seconds(flag: Option<u64>, configured: u64) -> Result<u64, String> {
    match flag {
        Some(0) => Err(
            "`--budget-seconds` is 0. The analysis clock must be at least 1 second. Pass --budget-seconds N with N at least 1."
                .into(),
        ),
        Some(seconds) => Ok(seconds),
        None => {
            validate_budget_seconds(configured)?;
            Ok(configured)
        }
    }
}

fn validate_budget_seconds(seconds: u64) -> Result<(), String> {
    if seconds == 0 {
        return Err(
            "`budget_seconds` is 0. The analysis clock must be at least 1 second. Set `budget_seconds = 120` in analyzer.toml, or omit the key."
                .into(),
        );
    }
    Ok(())
}

fn validate_rust_test_args(args: &[String]) -> Result<(), String> {
    for (index, arg) in args.iter().enumerate() {
        if arg.is_empty() {
            return Err(
                "rust.test_args contains an empty string. Each entry is one cargo or harness argument. In analyzer.toml use test_args = [\"--locked\", \"--features\", \"extra\", \"--\", \"--test-threads=1\"]."
                    .into(),
            );
        }
        if arg.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
            return Err(format!(
                "rust.test_args entry `{arg}` contains whitespace or a control character. Put each cargo argument in its own string. In analyzer.toml use test_args = [\"--locked\", \"--\", \"--test-threads=1\"]."
            ));
        }
        if arg == "--manifest-path" || arg.starts_with("--manifest-path=") {
            return Err(
                "rust.test_args cannot set --manifest-path. sc always passes the Cargo.toml of the directory being analyzed. Remove --manifest-path from test_args in analyzer.toml."
                    .into(),
            );
        }
        if arg == "--features=" || arg == "-F=" {
            return Err(
                "rust.test_args has an empty feature list. Write [\"--features\", \"name\"] or [\"--features=name\"] in analyzer.toml."
                    .into(),
            );
        }
        if arg == "--features" || arg == "-F" {
            let value = args.get(index + 1).map(String::as_str);
            let valid = value
                .is_some_and(|value| !value.is_empty() && value != "--" && !value.starts_with('-'));
            if !valid {
                return Err(
                    "rust.test_args has `--features` or `-F` without a feature list. Write [\"--features\", \"name\"] or [\"--features=name\"] in analyzer.toml."
                        .into(),
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn config_dir_is_dot_config_off_windows_and_appdata_on_windows() {
        let profile = Path::new("/u/me");
        assert_eq!(
            config_dir(profile, Some(Path::new("/ignored")), false),
            Path::new("/u/me/.config/sc")
        );
        assert_eq!(
            config_dir(profile, Some(Path::new("/roam")), true),
            Path::new("/roam/sc")
        );
        assert_eq!(
            config_dir(profile, None, true),
            Path::new("/u/me/AppData/Roaming/sc")
        );
        assert_eq!(
            config_dir(profile, Some(Path::new("")), true),
            Path::new("/u/me/AppData/Roaming/sc")
        );
    }

    #[test]
    fn appdata_applies_only_to_the_native_profile() {
        let os = |text: &str| Some(OsString::from(text));
        let native = Path::new("/u/me");
        assert_eq!(
            appdata_for_profile(native, os("/roam"), os("/u/me")),
            Some(PathBuf::from("/roam"))
        );
        assert_eq!(
            appdata_for_profile(Path::new("/tmp/home"), os("/roam"), os("/u/me")),
            None
        );
        assert_eq!(appdata_for_profile(native, None, os("/u/me")), None);
        assert_eq!(appdata_for_profile(native, os("/roam"), None), None);
    }

    #[test]
    fn profile_prefers_home_and_falls_back_to_userprofile_only_on_windows() {
        let os = |text: &str| Some(OsString::from(text));
        assert_eq!(profile_dir(os("/h"), os("/p"), true), Some("/h".into()));
        assert_eq!(profile_dir(None, os("/p"), true), Some("/p".into()));
        assert_eq!(profile_dir(os(""), os("/p"), true), Some("/p".into()));
        assert_eq!(profile_dir(None, os("/p"), false), None);
        assert_eq!(profile_dir(None, None, true), None);
    }

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
        assert!(!config.engines.parallel);
        assert!(!config.engines.perf);
        assert!(config.pack.is_empty());
        assert_eq!(config.gates.new_fn_untested_cc, 15);
        assert!(!config.llm.enabled);
        assert_eq!(config.mutation.mode, "off");
        assert_eq!(config.commands.lint, "cargo clippy --workspace");
        assert!(!config.commands.lint.contains("-D warnings"));
        assert_eq!(config.budget_seconds, 120);
        assert!(config.rust.test_args.is_empty());
    }

    #[test]
    fn omitted_budget_and_test_args_keep_the_historical_defaults() {
        let config: Config = toml::from_str("pack = \"rust\"\n").unwrap();
        assert_eq!(config.budget_seconds, 120);
        assert!(config.rust.test_args.is_empty());
        assert_eq!(
            resolve_budget_seconds(None, config.budget_seconds).unwrap(),
            120
        );
        let configured: Config =
            toml::from_str("budget_seconds = 900\n[rust]\ntest_args = [\"--locked\"]\n").unwrap();
        assert_eq!(configured.budget_seconds, 900);
        assert_eq!(configured.rust.test_args, vec!["--locked"]);
        assert_eq!(
            resolve_budget_seconds(None, configured.budget_seconds).unwrap(),
            900
        );
        assert_eq!(
            resolve_budget_seconds(Some(30), configured.budget_seconds).unwrap(),
            30
        );
        let loaded = load_config_file(None).unwrap();
        assert_eq!(loaded.budget_seconds, 120);
        assert!(loaded.rust.test_args.is_empty());
    }

    #[test]
    fn zero_budget_and_bad_test_args_name_the_config_line_to_fix() {
        let budget = validate_budget_seconds(0).unwrap_err();
        assert!(budget.contains("budget_seconds = 120"), "{budget}");
        assert!(budget.contains("at least 1"), "{budget}");
        assert!(validate_budget_seconds(1).is_ok());
        let flag = resolve_budget_seconds(Some(0), 120).unwrap_err();
        assert!(flag.contains("--budget-seconds"), "{flag}");
        assert!(flag.contains("at least 1"), "{flag}");

        let empty = validate_rust_test_args(&[String::new()]).unwrap_err();
        assert!(empty.contains("test_args = ["), "{empty}");
        let spaced = validate_rust_test_args(&["--locked --features extra".into()]).unwrap_err();
        assert!(spaced.contains("own string"), "{spaced}");
        let manifest =
            validate_rust_test_args(&["--manifest-path".into(), "other/Cargo.toml".into()])
                .unwrap_err();
        assert!(manifest.contains("Remove --manifest-path"), "{manifest}");
        let manifest_eq =
            validate_rust_test_args(&["--manifest-path=other/Cargo.toml".into()]).unwrap_err();
        assert!(
            manifest_eq.contains("Remove --manifest-path"),
            "{manifest_eq}"
        );
        let bare = validate_rust_test_args(&["--features".into()]).unwrap_err();
        assert!(bare.contains("[\"--features\", \"name\"]"), "{bare}");
        let flag_value =
            validate_rust_test_args(&["--features".into(), "--locked".into()]).unwrap_err();
        assert!(
            flag_value.contains("[\"--features\", \"name\"]"),
            "{flag_value}"
        );
        let empty_list = validate_rust_test_args(&["--features=".into()]).unwrap_err();
        assert!(empty_list.contains("empty feature list"), "{empty_list}");
        assert!(validate_rust_test_args(&["-F".into()]).is_err());
        assert!(validate_rust_test_args(&[
            "--locked".into(),
            "--features".into(),
            "extra".into(),
            "--".into(),
            "--test-threads=1".into(),
        ])
        .is_ok());
        assert!(validate_rust_test_args(&["--features=extra".into()]).is_ok());
        assert!(validate_rust_test_args(&["-F".into(), "extra".into()]).is_ok());

        let dir = std::env::temp_dir().join(format!("sc-cfg-budget-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("analyzer.toml");
        std::fs::write(&path, "budget_seconds = 0\n").unwrap();
        let loaded = load_config_file(Some(&path)).unwrap_err();
        assert!(loaded.contains("budget_seconds = 120"), "{loaded}");
        std::fs::write(
            &path,
            "[rust]\ntest_args = [\"--manifest-path\", \"other/Cargo.toml\"]\n",
        )
        .unwrap();
        let loaded = load_config_file(Some(&path)).unwrap_err();
        assert!(loaded.contains("Remove --manifest-path"), "{loaded}");
        let _ = std::fs::remove_dir_all(&dir);
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
        assert_eq!(config.budget_seconds, 120);
        assert!(config.rust.test_args.is_empty());
        let text = std::fs::read_to_string(&path).unwrap();
        let value: toml::Value = toml::from_str(&text).unwrap();
        assert_eq!(
            value
                .get("budget_seconds")
                .and_then(toml::Value::as_integer),
            Some(120),
            "budget_seconds must be a top-level key, not a table field"
        );
        let test_args = value
            .get("rust")
            .and_then(|rust| rust.get("test_args"))
            .and_then(toml::Value::as_array)
            .expect("rust.test_args array");
        assert!(test_args.is_empty());
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
