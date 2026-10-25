// SPDX-License-Identifier: MIT
//! Windows console VT mode and width for pretty CLI output.
//!
//! Matches Prismattyc `enable_console_vt` for line-oriented CLI output:
//! enable `ENABLE_VIRTUAL_TERMINAL_PROCESSING` on stdout and stderr, and clear
//! `DISABLE_NEWLINE_AUTO_RETURN` so a newline returns to column 0. Color follows
//! stdout accepting that mode. Width falls back to
//! `GetConsoleScreenBufferInfo` when `COLUMNS` is unset.

/// `ENABLE_VIRTUAL_TERMINAL_PROCESSING`.
pub const VT_OUTPUT_PROCESSING: u32 = 0x0004;
/// `DISABLE_NEWLINE_AUTO_RETURN`.
pub const VT_DISABLE_NEWLINE_AUTO_RETURN: u32 = 0x0008;

/// Line-oriented CLI output mode: VT processing on, auto-return kept so a
/// newline lands in column 0 (Prismattyc `cli_console_output_mode`).
pub fn cli_console_output_mode(mode: u32) -> u32 {
    (mode | VT_OUTPUT_PROCESSING) & !VT_DISABLE_NEWLINE_AUTO_RETURN
}

/// Color follows stdout. Enabling stderr must not make a rejected stdout
/// look ready.
pub fn stdout_vt_ready(stdout_accepted: bool, _stderr_accepted: bool) -> bool {
    stdout_accepted
}

/// Pure mode decision for tests and callers that inject handle results.
///
/// - `accepted`: console accepted the updated mode
/// - `rejected`: console handle exists but `SetConsoleMode` failed
/// - `pipe`: not a console (`GetConsoleMode` failed / invalid handle)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VtDecision {
    Accepted,
    Rejected,
    Pipe,
}

impl VtDecision {
    pub fn from_apply(got_mode: bool, set_ok: bool) -> Self {
        if !got_mode {
            return Self::Pipe;
        }
        if set_ok {
            Self::Accepted
        } else {
            Self::Rejected
        }
    }

    pub fn accepted(self) -> bool {
        matches!(self, Self::Accepted)
    }
}

/// Enable virtual-terminal sequences on a console CLI.
///
/// Returns true when stdout accepted output processing. Stderr is enabled too,
/// but its result does not decide color. A pipe, a file, or a rejected stdout
/// mode returns false. Unix terminals already interpret the sequences, so this
/// reports ready there.
pub fn enable_console_vt() -> bool {
    #[cfg(not(windows))]
    {
        true
    }
    #[cfg(windows)]
    {
        enable_console_vt_windows()
    }
}

#[cfg(windows)]
fn enable_console_vt_windows() -> bool {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{
        GetConsoleMode, GetStdHandle, SetConsoleMode, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
    };

    unsafe fn apply(handle_id: u32) -> VtDecision {
        let handle = GetStdHandle(handle_id);
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return VtDecision::Pipe;
        }
        let mut mode = 0u32;
        if GetConsoleMode(handle, &mut mode) == 0 {
            return VtDecision::Pipe;
        }
        let set_ok = SetConsoleMode(handle, cli_console_output_mode(mode)) != 0;
        VtDecision::from_apply(true, set_ok)
    }

    unsafe {
        let out = apply(STD_OUTPUT_HANDLE);
        let err = apply(STD_ERROR_HANDLE);
        stdout_vt_ready(out.accepted(), err.accepted())
    }
}

/// Console window width when `COLUMNS` is unset. Unix uses `TIOCGWINSZ`;
/// Windows uses `GetConsoleScreenBufferInfo`.
pub fn native_cols(min_width: usize) -> Option<usize> {
    #[cfg(unix)]
    {
        unix_cols(min_width)
    }
    #[cfg(windows)]
    {
        windows_cols(min_width)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = min_width;
        None
    }
}

#[cfg(unix)]
fn unix_cols(min_width: usize) -> Option<usize> {
    unsafe {
        // SAFETY: winsize is a plain C struct and the pointer is only
        // read by this ioctl for the life of the call.
        let mut size = libc::winsize {
            ws_row: 0,
            ws_col: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let rc = libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut size);
        if rc == 0 && size.ws_col as usize >= min_width {
            Some(size.ws_col as usize)
        } else {
            None
        }
    }
}

#[cfg(windows)]
fn windows_cols(min_width: usize) -> Option<usize> {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{
        GetConsoleScreenBufferInfo, GetStdHandle, CONSOLE_SCREEN_BUFFER_INFO, STD_OUTPUT_HANDLE,
    };

    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut info: CONSOLE_SCREEN_BUFFER_INFO = std::mem::zeroed();
        if GetConsoleScreenBufferInfo(handle, &mut info) == 0 {
            return None;
        }
        let width = (info.srWindow.Right - info.srWindow.Left + 1) as isize;
        if width >= min_width as isize {
            Some(width as usize)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_mode_enables_vt_and_clears_auto_return_disable() {
        assert_eq!(cli_console_output_mode(0), VT_OUTPUT_PROCESSING);
        assert_eq!(
            cli_console_output_mode(VT_OUTPUT_PROCESSING | VT_DISABLE_NEWLINE_AUTO_RETURN),
            VT_OUTPUT_PROCESSING
        );
        assert_eq!(
            cli_console_output_mode(0x0001 | 0x0002 | VT_DISABLE_NEWLINE_AUTO_RETURN),
            0x0001 | 0x0002 | VT_OUTPUT_PROCESSING
        );
    }

    #[test]
    fn mode_decision_covers_accepted_rejected_and_pipe() {
        assert_eq!(VtDecision::from_apply(true, true), VtDecision::Accepted);
        assert_eq!(VtDecision::from_apply(true, false), VtDecision::Rejected);
        assert_eq!(VtDecision::from_apply(false, true), VtDecision::Pipe);
        assert_eq!(VtDecision::from_apply(false, false), VtDecision::Pipe);
        assert!(VtDecision::Accepted.accepted());
        assert!(!VtDecision::Rejected.accepted());
        assert!(!VtDecision::Pipe.accepted());
    }

    #[test]
    fn stdout_vt_ready_ignores_stderr_accept() {
        assert!(!stdout_vt_ready(false, true));
        assert!(stdout_vt_ready(true, false));
        assert!(!stdout_vt_ready(false, false));
        assert!(stdout_vt_ready(true, true));
    }

    #[cfg(not(windows))]
    #[test]
    fn unix_terminals_already_accept_vt() {
        assert!(enable_console_vt());
    }

    #[cfg(windows)]
    #[test]
    fn vt_flags_match_the_sdk() {
        use windows_sys::Win32::System::Console::{
            DISABLE_NEWLINE_AUTO_RETURN, ENABLE_VIRTUAL_TERMINAL_PROCESSING,
        };
        assert_eq!(VT_OUTPUT_PROCESSING, ENABLE_VIRTUAL_TERMINAL_PROCESSING);
        assert_eq!(VT_DISABLE_NEWLINE_AUTO_RETURN, DISABLE_NEWLINE_AUTO_RETURN);
        assert_eq!(
            cli_console_output_mode(DISABLE_NEWLINE_AUTO_RETURN),
            ENABLE_VIRTUAL_TERMINAL_PROCESSING
        );
    }
}
