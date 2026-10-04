// SPDX-License-Identifier: MPL-2.0
//! Progress spinner for `sc analyze`.
//!
//! The spinner animates on stderr only, so stdout stays byte-identical
//! (JSON/SARIF output and pipes are unaffected). It runs only when stderr
//! is a TTY and the environment looks interactive; otherwise analysis stays
//! silent while it works.

use std::io::Write;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Braille frames, one per tick.
const FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// Delay between frames. The first frame renders immediately, so feedback
/// lands well within ~100 ms of the analysis start.
pub const TICK: Duration = Duration::from_millis(80);

/// Pure gating decision, kept side-effect free for tests.
pub fn should_show(
    stderr_tty: bool,
    quiet: bool,
    no_color: bool,
    term_dumb: bool,
    ci: bool,
    from_mcp: bool,
) -> bool {
    stderr_tty && !quiet && !no_color && !term_dumb && !ci && !from_mcp
}

/// Read the environment for the gating decision.
///
/// Silent when piped, in CI, with `NO_COLOR`/`TERM=dumb`, or when called
/// from `sc-mcp`, so agents and machine-readable output are unaffected.
pub fn should_show_from_env(stderr_tty: bool, quiet: bool) -> bool {
    let no_color = std::env::var_os("NO_COLOR").is_some();
    let term_dumb = matches!(std::env::var("TERM"), Ok(term) if term == "dumb");
    let ci = [
        "CI",
        "CONTINUOUS_INTEGRATION",
        "GITHUB_ACTIONS",
        "GITLAB_CI",
        "JENKINS_URL",
        "TEAMCITY_VERSION",
        "TF_BUILD",
    ]
    .iter()
    .any(|var| std::env::var_os(var).is_some());
    let from_mcp = std::env::var_os("SC_MCP").is_some();
    should_show(stderr_tty, quiet, no_color, term_dumb, ci, from_mcp)
}

/// Format elapsed time as `M:SS` (or `H:MM:SS` past an hour).
pub fn fmt_elapsed(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let secs = secs % 60;
    if hours > 0 {
        format!("{hours}:{mins:02}:{secs:02}")
    } else {
        format!("{mins}:{secs:02}")
    }
}

/// Render one spinner tick. The result carries `\r` plus a line clear and no
/// newline, so it rewrites the current stderr line in place.
pub fn frame_line(frame: char, step: &str, elapsed: Duration) -> String {
    format!("\r\x1b[2K{frame} {step} · {}", fmt_elapsed(elapsed))
}

fn clear_line() {
    let mut err = std::io::stderr().lock();
    let _ = write!(err, "\r\x1b[2K");
    let _ = err.flush();
}

/// Animated spinner writing to stderr on a background thread.
pub struct Spinner {
    step: Arc<Mutex<String>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Spinner {
    /// Start animating with the given step label. The first frame renders
    /// immediately on the calling thread.
    pub fn start(initial: &str) -> Self {
        let step = Arc::new(Mutex::new(initial.to_string()));
        let stop = Arc::new(AtomicBool::new(false));
        let started = Instant::now();
        {
            let mut err = std::io::stderr().lock();
            let _ = write!(err, "{}", frame_line(FRAMES[0], initial, started.elapsed()));
            let _ = err.flush();
        }
        let worker_step = Arc::clone(&step);
        let worker_stop = Arc::clone(&stop);
        let handle = std::thread::spawn(move || {
            let mut tick = 1usize;
            loop {
                for _ in 0..8 {
                    if worker_stop.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(TICK / 8);
                }
                if worker_stop.load(Ordering::Relaxed) {
                    return;
                }
                let label = worker_step
                    .lock()
                    .map(|label| label.clone())
                    .unwrap_or_default();
                let line = frame_line(FRAMES[tick % FRAMES.len()], &label, started.elapsed());
                {
                    let mut err = std::io::stderr().lock();
                    let _ = write!(err, "{line}");
                    let _ = err.flush();
                }
                tick += 1;
            }
        });
        Spinner {
            step,
            stop,
            handle: Some(handle),
        }
    }

    /// Build the engine progress callback that drives this spinner.
    pub fn updater(&self) -> sc_engines::ProgressCallback {
        let step = Arc::clone(&self.step);
        Arc::new(move |label: &str| {
            if let Ok(mut current) = step.lock() {
                *current = label.to_string();
            }
        })
    }

    /// Stop the spinner and clear its line. The report prints after this.
    pub fn finish(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        clear_line();
    }

    #[cfg(test)]
    fn current_step(&self) -> String {
        self.step
            .lock()
            .map(|label| label.clone())
            .unwrap_or_default()
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_only_on_an_interactive_stderr() {
        // (stderr_tty, quiet, no_color, term_dumb, ci, from_mcp)
        assert!(should_show(true, false, false, false, false, false));
        assert!(!should_show(false, false, false, false, false, false));
        assert!(!should_show(true, true, false, false, false, false));
        assert!(!should_show(true, false, true, false, false, false));
        assert!(!should_show(true, false, false, true, false, false));
        assert!(!should_show(true, false, false, false, true, false));
        assert!(!should_show(true, false, false, false, false, true));
    }

    #[test]
    fn elapsed_formats_as_minutes_and_seconds() {
        assert_eq!(fmt_elapsed(Duration::from_secs(0)), "0:00");
        assert_eq!(fmt_elapsed(Duration::from_secs(5)), "0:05");
        assert_eq!(fmt_elapsed(Duration::from_secs(42)), "0:42");
        assert_eq!(fmt_elapsed(Duration::from_secs(61)), "1:01");
        assert_eq!(fmt_elapsed(Duration::from_secs(754)), "12:34");
        assert_eq!(fmt_elapsed(Duration::from_secs(3723)), "1:02:03");
    }

    #[test]
    fn frame_rewrites_the_stderr_line_with_step_and_elapsed() {
        let line = frame_line('⠋', "Running tests (cargo test)", Duration::from_secs(42));
        assert!(line.starts_with("\r"), "must return to the line start");
        assert!(line.contains("Running tests (cargo test)"));
        assert!(line.contains("0:42"));
        assert!(!line.contains('\n'), "must never print a newline");
    }

    #[test]
    fn updater_replaces_the_step_label() {
        let spinner = Spinner::start("Detecting pack");
        assert_eq!(spinner.current_step(), "Detecting pack");
        spinner.updater()("Running tests (cargo test)");
        assert_eq!(spinner.current_step(), "Running tests (cargo test)");
        spinner.updater()("Writing report");
        assert_eq!(spinner.current_step(), "Writing report");
        spinner.finish();
    }
}
