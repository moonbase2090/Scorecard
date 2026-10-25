// SPDX-License-Identifier: MIT
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

#[cfg(windows)]
mod windows;

#[derive(Debug)]
pub struct Captured {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
    pub elapsed: Duration,
}

#[derive(Debug)]
pub enum CommandError {
    NotFound,
    Spawn(String),
    /// The budget was spent before the command could start.
    BudgetExhausted,
    /// The command was stopped at `limit`, the budget left when it started.
    Timeout {
        elapsed: Duration,
        limit: Duration,
        stderr: String,
    },
}

impl CommandError {
    /// The budget was already spent, so the command never ran.
    pub fn spent() -> Self {
        Self::BudgetExhausted
    }

    pub fn message(&self, what: &str) -> String {
        match self {
            Self::NotFound => format!("{what}: cargo is not installed or not on PATH"),
            Self::Spawn(err) => format!("{what}: {err}"),
            Self::BudgetExhausted => {
                format!("{what} did not run: earlier steps used the whole analysis budget")
            }
            Self::Timeout {
                elapsed,
                limit,
                stderr,
            } => {
                let mut message = format!(
                    "{what} timed out after {}s (the budget left for it was {}s)",
                    seconds(*elapsed),
                    seconds(*limit)
                );
                let tail = stderr_tail(stderr);
                if !tail.is_empty() {
                    message.push_str(&format!("; last stderr: {tail}"));
                }
                message
            }
        }
    }

    /// Exact next step for a timeout: run the command to see what it waits on,
    /// then rerun with a budget larger than this run's.
    pub fn timeout_fix(&self, what: &str) -> String {
        let spent = matches!(self, Self::BudgetExhausted);
        timeout_fix_text(what, spent, RUN_BUDGET_SECS.load(Ordering::Relaxed))
    }
}

const DEFAULT_BUDGET_SECS: u64 = 120;
static RUN_BUDGET_SECS: AtomicU64 = AtomicU64::new(DEFAULT_BUDGET_SECS);

/// Record the `--budget-seconds` of the run in progress so a timeout can
/// suggest a larger one.
pub fn set_run_budget(budget: Duration) {
    RUN_BUDGET_SECS.store(budget.as_secs().max(1), Ordering::Relaxed);
}

fn timeout_fix_text(what: &str, spent: bool, budget_secs: u64) -> String {
    let raised = budget_secs * 2;
    if spent {
        return format!(
            "Re-run with --budget-seconds {raised}, or find the step that used the budget up in the report's runs."
        );
    }
    format!(
        "Run `{what}` in the project to see what it waits on. If it only needs more time, re-run with --budget-seconds {raised}."
    )
}

fn seconds(duration: Duration) -> String {
    if duration.as_secs() >= 10 {
        duration.as_secs().to_string()
    } else {
        format!("{:.1}", duration.as_secs_f64())
    }
}

/// The last non-empty stderr lines, at most 300 bytes, on one line.
fn stderr_tail(stderr: &str) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let joined = lines[lines.len().saturating_sub(3)..].join(" | ");
    let mut start = joined.len().saturating_sub(300);
    while !joined.is_char_boundary(start) {
        start += 1;
    }
    joined[start..].to_string()
}

pub fn brief(text: &str) -> String {
    let mut out = String::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(4)
    {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(line);
        if out.len() > 400 {
            out.truncate(400);
            break;
        }
    }
    out
}

pub fn budget_left(deadline: Instant) -> Result<Duration, CommandError> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        Err(CommandError::spent())
    } else {
        Ok(left)
    }
}

pub fn cargo_command(root: &Path, config_pin: &str) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("CARGO_TERM_COLOR", "never")
        .env("CARGO_TARGET_DIR", root.join("target"))
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("LLVM_PROFILE_FILE")
        .env_remove("CARGO_MANIFEST_DIR")
        .env_remove("CARGO_MANIFEST_PATH");
    strip_parent_llvm_cov(&mut cmd);
    let policy = crate::rust_toolchain::resolve(root, config_pin);
    crate::rust_toolchain::apply(&mut cmd, &policy);
    cmd
}

const LLVM_COV_VARS: &[&str] = &[
    "CARGO_LLVM_COV",
    "CARGO_LLVM_COV_SHOW_ENV",
    "CARGO_LLVM_COV_TARGET_DIR",
    "CARGO_LLVM_COV_BUILD_DIR",
    "__CARGO_LLVM_COV_RUSTC_WRAPPER",
    "__CARGO_LLVM_COV_RUSTC_WRAPPER_RUSTFLAGS",
    "__CARGO_LLVM_COV_RUSTC_WRAPPER_CRATE_NAMES",
];

pub fn under_llvm_cov(vars: &[(&str, &str)]) -> bool {
    vars.iter().any(|(key, value)| match *key {
        "CARGO_LLVM_COV" => !value.is_empty(),
        "RUSTC_WRAPPER" => value.contains("llvm-cov"),
        _ => false,
    })
}

fn strip_parent_llvm_cov(cmd: &mut Command) {
    for key in LLVM_COV_VARS {
        cmd.env_remove(key);
    }
    let cov = std::env::var("CARGO_LLVM_COV").unwrap_or_default();
    let wrapper = std::env::var("RUSTC_WRAPPER").unwrap_or_default();
    if under_llvm_cov(&[
        ("CARGO_LLVM_COV", cov.as_str()),
        ("RUSTC_WRAPPER", wrapper.as_str()),
    ]) {
        cmd.env_remove("RUSTC_WRAPPER");
    }
}

pub fn run_cmd(cmd: &mut Command, timeout: Duration) -> Result<Captured, CommandError> {
    run_cmd_input(cmd, timeout, None)
}

pub fn run_cmd_input(
    cmd: &mut Command,
    timeout: Duration,
    input: Option<&[u8]>,
) -> Result<Captured, CommandError> {
    if timeout.is_zero() {
        return Err(CommandError::spent());
    }
    if input.is_some() {
        cmd.stdin(Stdio::piped());
    }
    #[cfg(unix)]
    {
        cmd.process_group(0);
    }
    #[cfg(windows)]
    let job = windows::Job::new()
        .map_err(|err| CommandError::Spawn(format!("create process job: {err}")))?;
    #[cfg(windows)]
    windows::suspend_on_spawn(cmd);
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err)
            if err.kind() == std::io::ErrorKind::NotFound
                && cmd.get_current_dir().is_none_or(|dir| dir.is_dir()) =>
        {
            return Err(CommandError::NotFound)
        }
        Err(err) => return Err(CommandError::Spawn(err.to_string())),
    };
    #[cfg(windows)]
    if let Err(err) = job.assign_and_resume(&child) {
        stop_tree(&mut child);
        return Err(CommandError::Spawn(format!("contain process tree: {err}")));
    }
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| CommandError::Spawn("stdout was not piped".into()))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| CommandError::Spawn("stderr was not piped".into()))?;
    let out_handle = thread::spawn(move || {
        let mut buf = String::new();
        let _ = stdout.read_to_string(&mut buf);
        buf
    });
    let err_handle = thread::spawn(move || {
        let mut buf = String::new();
        let _ = stderr.read_to_string(&mut buf);
        buf
    });
    if let Some(input) = input {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(input);
        }
    }

    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() >= timeout => {
                #[cfg(windows)]
                drop(job);
                stop_tree(&mut child);
                let _ = drain(out_handle);
                let stderr = drain(err_handle);
                return Err(CommandError::Timeout {
                    elapsed: start.elapsed(),
                    limit: timeout,
                    stderr,
                });
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(err) => {
                #[cfg(windows)]
                drop(job);
                stop_tree(&mut child);
                let _ = drain(out_handle);
                let _ = drain(err_handle);
                return Err(CommandError::Spawn(err.to_string()));
            }
        }
    };

    #[cfg(windows)]
    drop(job);
    let stdout = out_handle.join().unwrap_or_default();
    let stderr = err_handle.join().unwrap_or_default();
    Ok(Captured {
        status,
        stdout,
        stderr,
        elapsed: start.elapsed(),
    })
}

pub fn run_cargo(
    root: &Path,
    args: &[&str],
    deadline: Instant,
    config_pin: &str,
) -> Result<Captured, CommandError> {
    if !crate::toolchain::host_has("cargo") {
        let mut script = String::from("cargo");
        for arg in args {
            script.push(' ');
            script.push_str(&shell_quote_arg(arg));
        }
        // Docker image only has stable. A config pin is passed through so a
        // missing channel fails clearly; a file pin is left to rustup in the image.
        let policy = crate::rust_toolchain::resolve(root, config_pin);
        if let Some(pin) = crate::rust_toolchain::docker_env_prefix(&policy) {
            script = format!("RUSTUP_TOOLCHAIN={} {script}", shell_quote_arg(pin));
        }
        return crate::toolchain::run_script(root, &script, deadline);
    }
    let timeout = budget_left(deadline)?;
    let mut cmd = cargo_command(root, config_pin);
    cmd.args(args);
    run_cmd(&mut cmd, timeout)
}

pub(crate) fn shell_quote_arg(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// Kill the child and every process in its group, then stop waiting.
fn stop_tree(child: &mut Child) {
    #[cfg(unix)]
    unsafe {
        kill(-(child.id() as i32), 9);
    }
    let _ = child.kill();
    let grace = Instant::now() + Duration::from_millis(500);
    loop {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => break,
            Ok(None) if Instant::now() >= grace => break,
            Ok(None) => thread::sleep(Duration::from_millis(10)),
        }
    }
}

fn drain(handle: thread::JoinHandle<String>) -> String {
    let grace = Instant::now() + Duration::from_millis(200);
    while !handle.is_finished() && Instant::now() < grace {
        thread::sleep(Duration::from_millis(10));
    }
    if handle.is_finished() {
        handle.join().unwrap_or_default()
    } else {
        drop(handle);
        String::new()
    }
}

#[cfg(unix)]
unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

#[cfg(test)]
mod tests {
    use super::under_llvm_cov;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    #[test]
    fn nested_llvm_cov_is_detected_from_its_wrapper() {
        assert!(under_llvm_cov(&[("CARGO_LLVM_COV", "1")]));
        assert!(under_llvm_cov(&[(
            "RUSTC_WRAPPER",
            "/usr/local/bin/cargo-llvm-cov",
        )]));
        assert!(!under_llvm_cov(&[("CARGO_LLVM_COV", "")]));
        assert!(!under_llvm_cov(&[("RUSTC_WRAPPER", "/usr/bin/sccache")]));
        assert!(!under_llvm_cov(&[]));
    }

    #[test]
    fn timeout_message_names_the_command_the_clock_and_the_last_stderr() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg("echo 'warming up' >&2; echo 'stuck on ._lib.rs' >&2; sleep 30")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let err = super::run_cmd(&mut cmd, Duration::from_millis(500)).unwrap_err();
        let message = err.message("cargo test --workspace");
        assert!(
            message.starts_with("cargo test --workspace timed out after 0.")
                && message.contains("s (the budget left for it was 0.5s); last stderr: "),
            "{message}"
        );
        assert!(
            message.ends_with("last stderr: warming up | stuck on ._lib.rs"),
            "{message}"
        );
        assert!(err
            .timeout_fix("cargo test --workspace")
            .starts_with("Run `cargo test --workspace` in the project to see what it waits on."));
    }

    #[test]
    fn suggested_budget_is_twice_the_budget_of_the_run() {
        assert!(
            super::timeout_fix_text("cargo test", false, 900).ends_with("--budget-seconds 1800.")
        );
        assert!(
            super::timeout_fix_text("cargo test", false, 120).ends_with("--budget-seconds 240.")
        );
        assert!(super::timeout_fix_text("cargo test", true, 900).contains("--budget-seconds 1800,"));
    }

    #[test]
    fn spent_budget_is_not_reported_as_a_slow_command() {
        let root = std::env::temp_dir().join(format!("sc-no-spawn-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let marker = root.join("started");
        let _ = std::fs::remove_file(&marker);
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg("echo started > started")
            .current_dir(&root);
        let err = super::run_cmd(&mut cmd, Duration::ZERO).unwrap_err();
        assert!(matches!(err, super::CommandError::BudgetExhausted));
        assert!(!marker.exists());
        let message = err.message("pytest");
        assert_eq!(
            message,
            "pytest did not run: earlier steps used the whole analysis budget"
        );
        assert!(super::CommandError::spent()
            .timeout_fix("pytest")
            .starts_with("Re-run with --budget-seconds "));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stderr_tail_keeps_the_last_three_lines_within_300_bytes() {
        let text = "a\n\nb\nc\nd\n";
        assert_eq!(super::stderr_tail(text), "b | c | d");
        let long = format!("{}é", "x".repeat(400));
        let tail = super::stderr_tail(&long);
        assert!(tail.len() <= 300 && tail.ends_with('é'), "{}", tail.len());
    }

    #[test]
    fn timeout_kills_a_grandchild_that_holds_the_pipe() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg("sleep 30 & echo $!; wait")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let start = Instant::now();
        let err = super::run_cmd(&mut cmd, Duration::from_millis(300)).unwrap_err();
        assert!(matches!(err, super::CommandError::Timeout { .. }));
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(2),
            "budget wait ran for {elapsed:?}"
        );
    }

    #[test]
    fn a_finished_command_still_returns_its_output() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg("echo ready")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let captured = super::run_cmd(&mut cmd, Duration::from_secs(5)).unwrap();
        assert!(captured.status.success());
        assert!(captured.stdout.contains("ready"));
    }
}
