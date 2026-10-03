// SPDX-License-Identifier: MPL-2.0
//! Install the embedded Scorecard skill for coding agents.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Args, Subcommand, ValueEnum};

const SKILL: &str = include_str!("../../../skills/scorecard/SKILL.md");
const OPT_OUT_ENV: &str = "SCORECARD_NO_AGENT_SKILLS";
const VERSION_FILE: &str = "agent-skills-version";
const SKILL_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Subcommand)]
pub(crate) enum SkillsCommand {
    /// Install the Scorecard skill for coding agents.
    Install(InstallArgs),
}

#[derive(Args)]
pub(crate) struct InstallArgs {
    /// Agent to install for. `detected` only targets agents already configured or on PATH.
    #[arg(long, value_enum, default_value_t = Agent::All)]
    agent: Agent,
    /// Report whether the skill is current without writing files.
    #[arg(long, conflicts_with = "force")]
    check: bool,
    /// Replace an existing skill, including an edited copy.
    #[arg(long)]
    force: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum Agent {
    All,
    Codex,
    Shared,
    Claude,
    Cursor,
    Kiro,
    Muse,
    Detected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileState {
    Current,
    Installed,
    Preserved,
    Missing,
    Changed,
}

pub(crate) fn run(command: SkillsCommand) -> i32 {
    match command {
        SkillsCommand::Install(args) => run_install(args),
    }
}

fn run_install(args: InstallArgs) -> i32 {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        eprintln!("HOME is not set; cannot resolve agent skill locations.");
        return 2;
    };
    let config_home = config_home(&home);
    let path = path_dirs();
    let env_opt_out = std::env::var_os(OPT_OUT_ENV).as_deref() == Some(std::ffi::OsStr::new("1"));
    run_install_at(&home, args, env_opt_out, &config_home, &path)
}

#[inline(never)]
fn run_install_at(
    home: &Path,
    args: InstallArgs,
    env_opt_out: bool,
    config_home: &Path,
    path: &[PathBuf],
) -> i32 {
    if args.agent == Agent::Detected {
        if let Some(reason) = automatic_install_opt_out(home, env_opt_out) {
            println!("Automatic agent skill installation skipped: {reason}");
            return 0;
        }
        let version_file = version_file(home);
        if !args.check && !args.force && version_matches(&version_file) {
            println!(
                "Scorecard {} agent skills are already installed.",
                SKILL_VERSION
            );
            return 0;
        }
    }

    let agents = match args.agent {
        Agent::All => vec![
            Agent::Codex,
            Agent::Shared,
            Agent::Claude,
            Agent::Cursor,
            Agent::Kiro,
            Agent::Muse,
        ],
        Agent::Codex => vec![Agent::Codex, Agent::Shared],
        Agent::Detected => detect_agents(home, config_home, path),
        agent => vec![agent],
    };
    if agents.is_empty() {
        println!("No configured coding agents were detected (no supported config directory or CLI on PATH), so no agent directories were created. Set up an agent and rerun `sc skills install --agent detected`, or run `sc skills install --agent all`.");
        return 0;
    }

    let mut failed = false;
    let mut needs_update = false;
    for agent in agents {
        let result = if agent == Agent::Muse {
            install_muse(home, config_home, path, args.check, args.force)
        } else {
            let Some(dir) = agent_dir(home, agent) else {
                eprintln!("Unsupported agent target: {agent:?}");
                failed = true;
                continue;
            };
            match write_skill(&dir, args.check, args.force) {
                Ok(state) => Ok((dir.join("SKILL.md"), state)),
                Err(err) => Err(err),
            }
        };
        match result {
            Ok((path, state)) => {
                report_state(agent, state, &path, args.check);
                needs_update |= matches!(state, FileState::Missing | FileState::Changed);
            }
            Err(err) => {
                eprintln!("{} skill: {err}", agent_name(agent));
                failed = true;
            }
        }
    }
    if args.check {
        return if failed || needs_update { 1 } else { 0 };
    }
    if failed {
        return 1;
    }
    if args.agent == Agent::Detected {
        return record_version(home);
    }
    0
}

#[inline(never)]
fn agent_name(agent: Agent) -> &'static str {
    match agent {
        Agent::All => "all",
        Agent::Codex => "Codex",
        Agent::Shared => "shared",
        Agent::Claude => "Claude",
        Agent::Cursor => "Cursor",
        Agent::Kiro => "Kiro",
        Agent::Muse => "Muse",
        Agent::Detected => "detected",
    }
}

#[inline(never)]
fn agent_dir(home: &Path, agent: Agent) -> Option<PathBuf> {
    let path = match agent {
        Agent::Codex => ".codex/skills/scorecard",
        Agent::Shared => ".agents/skills/scorecard",
        Agent::Claude => ".claude/skills/scorecard",
        Agent::Cursor => ".cursor/skills/scorecard",
        Agent::Kiro => ".kiro/skills/scorecard",
        Agent::All | Agent::Muse | Agent::Detected => return None,
    };
    Some(home.join(path))
}

fn agent_config_dir(home: &Path, agent: Agent) -> Option<PathBuf> {
    let path = match agent {
        Agent::Codex => ".codex",
        Agent::Shared => ".agents",
        Agent::Claude => ".claude",
        Agent::Cursor => ".cursor",
        Agent::Kiro => ".kiro",
        Agent::All | Agent::Muse | Agent::Detected => return None,
    };
    Some(home.join(path))
}

fn detect_agents(home: &Path, config_home: &Path, path: &[PathBuf]) -> Vec<Agent> {
    let mut detected = Vec::new();
    let codex_dir = home.join(".codex");
    let shared_dir = home.join(".agents");
    let codex_detected =
        codex_dir.is_dir() || shared_dir.is_dir() || executable_on_path("codex", path);
    if codex_detected {
        if codex_dir.is_dir() {
            detected.push(Agent::Codex);
        }
        if shared_dir.is_dir() {
            detected.push(Agent::Shared);
        }
        if !codex_dir.is_dir() && !shared_dir.is_dir() {
            detected.push(Agent::Codex);
        }
    }
    for (agent, binaries) in [
        (Agent::Claude, &["claude"][..]),
        (Agent::Cursor, &["cursor-agent", "cursor"][..]),
        (Agent::Kiro, &["kiro-cli", "kiro"][..]),
        (Agent::Muse, &["muse"][..]),
    ] {
        let config_exists = if agent == Agent::Muse {
            muse_agent_config_dirs(home, config_home)
                .iter()
                .any(|dir| dir.is_dir())
        } else {
            agent_config_dir(home, agent).is_some_and(|dir| dir.is_dir())
        };
        if config_exists || binaries.iter().any(|bin| executable_on_path(bin, path)) {
            detected.push(agent);
        }
    }
    detected
}

fn path_dirs() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default()
}

fn executable_on_path(name: &str, path: &[PathBuf]) -> bool {
    find_executable(name, path).is_some()
}

fn config_home(home: &Path) -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"))
}

fn muse_agent_config_dirs(home: &Path, config_home: &Path) -> Vec<PathBuf> {
    vec![config_home.join("muse"), home.join(".muse")]
}

#[inline(never)]
fn automatic_install_opt_out(home: &Path, env_opt_out: bool) -> Option<String> {
    if env_opt_out {
        return Some(format!(
            "{OPT_OUT_ENV}=1 is set; unset it to enable automatic installation, or run `sc skills install --agent codex` for a manual install."
        ));
    }
    let config_path = sc_core::user_config_file(home);
    if !config_path.is_file() {
        return None;
    }
    match sc_core::load_config_file(Some(&config_path)) {
        Ok(config) if config.install_agent_skills => None,
        Ok(_) => Some(format!(
            "`install_agent_skills = false` is set in {}; change it to `true`, or run `sc skills install --agent codex` for a manual install.",
            config_path.display()
        )),
        Err(err) => Some(format!(
            "could not read {}: {err}; repair that config or run `sc skills install --agent codex` for a manual install.",
            config_path.display()
        )),
    }
}

fn version_file(home: &Path) -> PathBuf {
    home.join(".config").join("sc").join(VERSION_FILE)
}

fn version_matches(path: &Path) -> bool {
    fs::read_to_string(path)
        .map(|version| version.trim() == SKILL_VERSION)
        .unwrap_or(false)
}

fn record_version(home: &Path) -> i32 {
    let path = version_file(home);
    let result = (|| -> Result<(), String> {
        let parent = path.parent().ok_or("version file has no parent")?;
        fs::create_dir_all(parent).map_err(|err| format!("{}: {err}", parent.display()))?;
        atomic_write(&path, SKILL_VERSION.as_bytes())
            .map_err(|err| format!("{}: {err}", path.display()))
    })();
    match result {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("Could not record the agent skill version: {err}. Retry with `sc skills install --agent detected`.");
            1
        }
    }
}

fn write_skill(dir: &Path, check: bool, force: bool) -> Result<FileState, String> {
    let path = dir.join("SKILL.md");
    if path.is_file() {
        let current = fs::read(&path).map_err(|err| format!("{}: {err}", path.display()))?;
        if current == SKILL.as_bytes() {
            return Ok(FileState::Current);
        }
        if check {
            return Ok(FileState::Changed);
        }
        if !force {
            return Ok(FileState::Preserved);
        }
    } else if check {
        return Ok(FileState::Missing);
    }
    fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    atomic_write(&path, SKILL.as_bytes()).map_err(|err| format!("{}: {err}", path.display()))?;
    Ok(FileState::Installed)
}

pub(super) fn write_preserving_skill(dir: &Path) -> Result<PathBuf, String> {
    let path = dir.join("SKILL.md");
    if path.is_file() {
        return Ok(path);
    }
    fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    atomic_write(&path, SKILL.as_bytes()).map_err(|err| format!("{}: {err}", path.display()))?;
    Ok(path)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp = parent.join(format!(
        ".scorecard-skill-{}-{nonce}.tmp",
        std::process::id()
    ));
    fs::write(&temp, bytes)?;
    if let Err(err) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(err);
    }
    Ok(())
}

#[inline(never)]
fn install_muse(
    home: &Path,
    config_home: &Path,
    path: &[PathBuf],
    check: bool,
    force: bool,
) -> Result<(PathBuf, FileState), String> {
    let executable = find_executable("muse", path)
        .ok_or("`muse` is not on PATH; install Muse or omit `--agent muse`")?;
    let existing = muse_skill_path(home, config_home, &executable);
    if let Some(path) = existing.as_ref() {
        let content = fs::read(path).map_err(|err| format!("{}: {err}", path.display()))?;
        if content == SKILL.as_bytes() {
            return Ok((path.clone(), FileState::Current));
        }
        if check {
            return Ok((path.clone(), FileState::Changed));
        }
        if !force {
            return Ok((path.clone(), FileState::Preserved));
        }
    } else if check {
        return Ok((
            home.join(".agents/skills/scorecard/SKILL.md"),
            FileState::Missing,
        ));
    }

    let temp_root = std::env::temp_dir().join(format!(
        "sc-muse-skill-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let source = temp_root.join("scorecard");
    fs::create_dir_all(&source).map_err(|err| format!("{}: {err}", source.display()))?;
    let source_skill = source.join("SKILL.md");
    if let Err(err) = fs::write(&source_skill, SKILL) {
        let _ = fs::remove_dir_all(&temp_root);
        return Err(format!("{}: {err}", source_skill.display()));
    }
    let mut command = Command::new(&executable);
    command.args(["skills", "install"]).arg(&source).args([
        "--scope",
        "user",
        "--name",
        "scorecard",
    ]);
    if force {
        command.arg("--force");
    }
    command.env("HOME", home);
    let output = command
        .output()
        .map_err(|err| format!("could not run muse: {err}"));
    let _ = fs::remove_dir_all(&temp_root);
    let output = output?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = if stderr.trim().is_empty() {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        } else {
            stderr.trim().to_string()
        };
        return Err(format!("muse skills install failed: {detail}"));
    }
    let installed = muse_skill_path(home, config_home, &executable)
        .unwrap_or_else(|| home.join(".agents/skills/scorecard/SKILL.md"));
    Ok((installed, FileState::Installed))
}

fn find_executable(name: &str, path: &[PathBuf]) -> Option<PathBuf> {
    path.iter()
        .map(|dir| dir.join(name))
        .find(|path| is_executable_file(path))
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

#[inline(never)]
fn muse_skill_path(home: &Path, config_home: &Path, executable: &Path) -> Option<PathBuf> {
    let output = Command::new(executable)
        .args(["skills", "list", "--source", "user", "--json"])
        .env("HOME", home)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let mut paths = Vec::new();
    if let Some(skills) = value.get("skills").and_then(serde_json::Value::as_array) {
        for skill in skills {
            if skill.get("id").and_then(serde_json::Value::as_str) == Some("scorecard")
                || skill.get("name").and_then(serde_json::Value::as_str) == Some("scorecard")
            {
                if let Some(path) = skill.get("path").and_then(serde_json::Value::as_str) {
                    paths.push(path);
                }
            }
        }
    }
    if let Some(diagnostics) = value
        .get("diagnostics")
        .and_then(serde_json::Value::as_array)
    {
        for diagnostic in diagnostics {
            let path = diagnostic.get("path").and_then(serde_json::Value::as_str);
            if path.is_some_and(|path| path.contains("/scorecard/")) {
                paths.extend(path);
            }
        }
    }
    paths.into_iter().find_map(|path| {
        let resolved = path.replace("$HOME", &home.display().to_string()).replace(
            "$CONFIG_DIR",
            &muse_agent_config_dirs(home, config_home)
                .first()?
                .display()
                .to_string(),
        );
        let path = PathBuf::from(resolved);
        path.is_file().then_some(path)
    })
}

fn report_state(agent: Agent, state: FileState, path: &Path, check: bool) {
    let name = agent_name(agent).to_ascii_lowercase();
    match state {
        FileState::Current => println!("current {}", path.display()),
        FileState::Installed => println!("installed {}", path.display()),
        FileState::Preserved => println!(
            "preserved edited copy {}; run `sc skills install --agent {name} --force` to replace it.",
            path.display()
        ),
        FileState::Missing if check => println!(
            "missing {}; run `sc skills install --agent {name}` to install it.",
            path.display()
        ),
        FileState::Changed if check => println!(
            "out of date or edited {}; review it, then run `sc skills install --agent {name} --force` to replace it.",
            path.display()
        ),
        FileState::Missing => println!("missing {}", path.display()),
        FileState::Changed => println!("out of date or edited {}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn temp_home() -> PathBuf {
        let home = std::env::temp_dir().join(format!(
            "sc-skills-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(&home).unwrap();
        home
    }

    fn args(agent: Agent) -> InstallArgs {
        InstallArgs {
            agent,
            check: false,
            force: false,
        }
    }

    fn fake_muse(home: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let bin = home.join("bin");
        fs::create_dir_all(&bin).unwrap();
        let executable = bin.join("muse");
        fs::write(
            &executable,
            r##"#!/bin/sh
destination="$HOME/.config/muse/skills/scorecard/SKILL.md"
if [ "$2" = "list" ]; then
  if [ -f "$destination" ]; then
    printf '%s\n' '{"skills":[{"id":"scorecard","path":"$CONFIG_DIR/skills/scorecard/SKILL.md"}],"diagnostics":[]}'
  else
    printf '%s\n' '{"skills":[],"diagnostics":[]}'
  fi
  exit 0
fi
mkdir -p "$(dirname "$destination")"
cp "$3/SKILL.md" "$destination"
"##,
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        executable
    }

    #[test]
    fn all_agents_have_names_and_file_targets() {
        for (agent, name, has_directory) in [
            (Agent::All, "all", false),
            (Agent::Codex, "Codex", true),
            (Agent::Shared, "shared", true),
            (Agent::Claude, "Claude", true),
            (Agent::Cursor, "Cursor", true),
            (Agent::Kiro, "Kiro", true),
            (Agent::Muse, "Muse", false),
            (Agent::Detected, "detected", false),
        ] {
            assert_eq!(agent_name(agent), name);
            assert_eq!(
                agent_dir(Path::new("/tmp/home"), agent).is_some(),
                has_directory
            );
        }
    }

    #[test]
    fn detects_only_existing_agent_configs_and_path_tools() {
        let home = temp_home();
        fs::create_dir_all(home.join(".claude")).unwrap();
        fs::create_dir_all(home.join(".agents")).unwrap();
        let bin = home.join("bin");
        fs::create_dir_all(&bin).unwrap();
        let fake_cli = bin.join("codex");
        fs::write(&fake_cli, "fake").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&fake_cli, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let unexecutable_cursor = bin.join("cursor");
        fs::write(&unexecutable_cursor, "not executable").unwrap();
        let cursor_agent = bin.join("cursor-agent");
        fs::write(&cursor_agent, "fake").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&cursor_agent, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let detected = detect_agents(&home, &home.join(".config"), std::slice::from_ref(&bin));
        assert_eq!(detected, vec![Agent::Shared, Agent::Claude, Agent::Cursor]);
        assert!(!home.join(".cursor").exists());
        assert!(!home.join(".kiro").exists());
        let codex_home = temp_home();
        let codex_only = detect_agents(
            &codex_home,
            &codex_home.join(".config"),
            std::slice::from_ref(&bin),
        );
        assert!(codex_only.contains(&Agent::Codex));
        let _ = fs::remove_dir_all(codex_home);
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn installer_preserves_edited_copy_until_forced() {
        let home = temp_home();
        let target = home.join(".codex/skills/scorecard");
        assert_eq!(
            write_skill(&target, false, false).unwrap(),
            FileState::Installed
        );
        let skill = target.join("SKILL.md");
        fs::write(&skill, "user edits\n").unwrap();
        assert_eq!(
            write_skill(&target, false, false).unwrap(),
            FileState::Preserved
        );
        assert_eq!(fs::read_to_string(&skill).unwrap(), "user edits\n");
        assert_eq!(
            write_skill(&target, false, true).unwrap(),
            FileState::Installed
        );
        assert_eq!(fs::read_to_string(&skill).unwrap(), SKILL);
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn check_does_not_create_agent_directories() {
        let home = temp_home();
        let target = home.join(".kiro/skills/scorecard");
        assert_eq!(
            write_skill(&target, true, false).unwrap(),
            FileState::Missing
        );
        assert!(!home.join(".kiro").exists());
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn config_opt_out_is_parsed() {
        let home = temp_home();
        let path = sc_core::user_config_file(&home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "install_agent_skills = false\n").unwrap();
        assert!(automatic_install_opt_out(&home, false)
            .unwrap()
            .contains("install_agent_skills = false"));
        assert!(automatic_install_opt_out(&home, true)
            .unwrap()
            .contains("SCORECARD_NO_AGENT_SKILLS=1"));
        let config = sc_core::load_config_file(Some(&path)).unwrap();
        assert!(!config.install_agent_skills);
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn detected_install_records_binary_version_and_skips_same_version() {
        let home = temp_home();
        fs::create_dir_all(home.join(".codex")).unwrap();
        let bin = home.join("bin");
        fs::create_dir_all(&bin).unwrap();
        let cursor = bin.join("cursor-agent");
        fs::write(&cursor, "fake").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&cursor, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let config = home.join(".config");
        assert_eq!(
            run_install_at(
                &home,
                args(Agent::Detected),
                false,
                &config,
                std::slice::from_ref(&bin),
            ),
            0
        );
        assert!(home.join(".codex/skills/scorecard/SKILL.md").is_file());
        assert!(home.join(".cursor/skills/scorecard/SKILL.md").is_file());
        assert!(!home.join(".agents").exists());
        assert_eq!(
            fs::read_to_string(version_file(&home)).unwrap(),
            SKILL_VERSION
        );

        fs::write(version_file(&home), "0.0.0").unwrap();
        assert_eq!(
            run_install_at(
                &home,
                args(Agent::Detected),
                false,
                &config,
                std::slice::from_ref(&bin),
            ),
            0
        );
        assert_eq!(
            fs::read_to_string(version_file(&home)).unwrap(),
            SKILL_VERSION
        );
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn opt_out_and_no_agents_do_not_record_a_version() {
        let home = temp_home();
        let config = home.join(".config");
        let bin = home.join("bin");
        fs::create_dir_all(&bin).unwrap();
        assert_eq!(
            run_install_at(&home, args(Agent::Detected), true, &config, &[]),
            0
        );
        assert!(!version_file(&home).exists());

        assert_eq!(
            run_install_at(&home, args(Agent::Detected), false, &config, &[]),
            0
        );
        assert!(!version_file(&home).exists());
        fs::create_dir_all(home.join(".claude")).unwrap();
        assert_eq!(
            run_install_at(&home, args(Agent::Detected), false, &config, &[]),
            0
        );
        assert!(home.join(".claude/skills/scorecard/SKILL.md").is_file());
        assert!(version_file(&home).is_file());
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn all_installs_file_agents_and_muse() {
        let home = temp_home();
        let executable = fake_muse(&home);
        let config = home.join(".config");
        let path = vec![executable.parent().unwrap().to_path_buf()];
        assert_eq!(
            run_install_at(&home, args(Agent::All), false, &config, &path),
            0
        );
        for rel in [
            ".codex/skills/scorecard/SKILL.md",
            ".agents/skills/scorecard/SKILL.md",
            ".claude/skills/scorecard/SKILL.md",
            ".cursor/skills/scorecard/SKILL.md",
            ".kiro/skills/scorecard/SKILL.md",
            ".config/muse/skills/scorecard/SKILL.md",
        ] {
            assert!(home.join(rel).is_file(), "missing {rel}");
        }
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn muse_install_preserves_edits_and_force_replaces_them() {
        let home = temp_home();
        let executable = fake_muse(&home);
        let config = home.join(".config");
        let path = vec![executable.parent().unwrap().to_path_buf()];
        let installed = home.join(".config/muse/skills/scorecard/SKILL.md");
        assert_eq!(
            install_muse(&home, &config, &path, false, false).unwrap().1,
            FileState::Installed
        );
        fs::write(&installed, "user edit\n").unwrap();
        assert_eq!(
            install_muse(&home, &config, &path, false, false).unwrap().1,
            FileState::Preserved
        );
        assert_eq!(fs::read_to_string(&installed).unwrap(), "user edit\n");
        assert_eq!(
            install_muse(&home, &config, &path, false, true).unwrap().1,
            FileState::Installed
        );
        assert_eq!(fs::read_to_string(&installed).unwrap(), SKILL);
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn report_state_handles_check_remediation_and_current_files() {
        let path = Path::new("/tmp/scorecard-skill/SKILL.md");
        report_state(Agent::Codex, FileState::Current, path, false);
        report_state(Agent::Shared, FileState::Installed, path, false);
        report_state(Agent::Codex, FileState::Preserved, path, false);
        report_state(Agent::Cursor, FileState::Missing, path, true);
        report_state(Agent::Muse, FileState::Changed, path, true);
        report_state(Agent::Kiro, FileState::Missing, path, false);
        report_state(Agent::Claude, FileState::Changed, path, false);
    }
}
