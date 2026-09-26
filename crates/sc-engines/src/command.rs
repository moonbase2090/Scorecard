use std::io::Read;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

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
    Timeout,
}

impl CommandError {
    pub fn message(&self, what: &str) -> String {
        match self {
            Self::NotFound => format!("{what}: cargo is not installed or not on PATH"),
            Self::Spawn(err) => format!("{what}: {err}"),
            Self::Timeout => format!("{what} timed out"),
        }
    }
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
        Err(CommandError::Timeout)
    } else {
        Ok(left)
    }
}

pub fn cargo_command(root: &Path) -> Command {
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
    cmd
}

pub fn run_cmd(cmd: &mut Command, timeout: Duration) -> Result<Captured, CommandError> {
    if timeout.is_zero() {
        return Err(CommandError::Timeout);
    }
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(CommandError::NotFound)
        }
        Err(err) => return Err(CommandError::Spawn(err.to_string())),
    };
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

    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = out_handle.join();
                let _ = err_handle.join();
                return Err(CommandError::Timeout);
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = out_handle.join();
                let _ = err_handle.join();
                return Err(CommandError::Spawn(err.to_string()));
            }
        }
    };

    let stdout = out_handle.join().unwrap_or_default();
    let stderr = err_handle.join().unwrap_or_default();
    Ok(Captured {
        status,
        stdout,
        stderr,
        elapsed: start.elapsed(),
    })
}

pub fn run_cargo(root: &Path, args: &[&str], deadline: Instant) -> Result<Captured, CommandError> {
    if !crate::toolchain::host_has("cargo") {
        let mut script = String::from("cargo");
        for arg in args {
            script.push(' ');
            script.push_str(&shell_quote_arg(arg));
        }
        return crate::toolchain::run_script(root, &script, deadline);
    }
    let timeout = budget_left(deadline)?;
    let mut cmd = cargo_command(root);
    cmd.args(args);
    run_cmd(&mut cmd, timeout)
}

fn shell_quote_arg(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}
