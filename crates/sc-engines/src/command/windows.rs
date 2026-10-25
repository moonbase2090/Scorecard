// SPDX-License-Identifier: MIT
use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};
use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{
    OpenThread, ResumeThread, CREATE_SUSPENDED, THREAD_SUSPEND_RESUME,
};

pub(super) struct Job(OwnedHandle);

impl Job {
    pub(super) fn new() -> io::Result<Self> {
        // SAFETY: Null arguments create an unnamed, non-inheritable job.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        let job = Self(owned(handle)?);
        // SAFETY: The all-zero structure has no limits until LimitFlags is set.
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: The handle and structure are live for the call; size matches its class.
        let ok = unsafe {
            SetInformationJobObject(
                job.0.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }

    pub(super) fn assign_and_resume(&self, child: &Child) -> io::Result<()> {
        // SAFETY: Both handles remain live throughout assignment.
        if unsafe { AssignProcessToJobObject(self.0.as_raw_handle(), child.as_raw_handle()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        resume(child.id())
    }
}

pub(super) fn suspend_on_spawn(cmd: &mut Command) {
    // Assignment must precede execution, or a fast child can spawn outside the job.
    cmd.creation_flags(CREATE_SUSPENDED);
}

fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: Each successful Win32 call transfers one owned handle to this function.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

fn resume(pid: u32) -> io::Result<()> {
    // std::process drops the primary thread handle. A suspended process cannot
    // create more threads, so the snapshot identifies its initial thread.
    // SAFETY: This call has no pointer parameters; owned rejects the error sentinel.
    let snapshot = owned(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) })?;
    // SAFETY: THREADENTRY32 is initialized before passing its pointer to Win32.
    let mut entry: THREADENTRY32 = unsafe { zeroed() };
    entry.dwSize = size_of::<THREADENTRY32>() as u32;
    // SAFETY: snapshot is live and entry has the required size field.
    let mut found = unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) };
    while found != 0 {
        if entry.th32OwnerProcessID == pid {
            // SAFETY: The thread ID came from the snapshot. The handle is not inherited.
            let thread =
                owned(unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) })?;
            // SAFETY: The owned thread handle grants THREAD_SUSPEND_RESUME.
            if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
                return Err(io::Error::last_os_error());
            }
            return Ok(());
        }
        // SAFETY: snapshot and entry remain valid for enumeration.
        found = unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) };
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "suspended child thread was not found",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{run_cmd, CommandError};
    use std::path::{Path, PathBuf};
    use std::process::Stdio;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{ERROR_INVALID_PARAMETER, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    };

    const FIXTURE: &str = "command::windows::tests::process_fixture";

    fn fixture(root: &Path, role: &str) -> Command {
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(["--ignored", "--exact", FIXTURE, "--nocapture"])
            .env("SC_JOB_FIXTURE_ROOT", root)
            .env("SC_JOB_FIXTURE_ROLE", role);
        cmd
    }

    #[test]
    #[ignore = "subprocess fixture, invoked by the job tests"]
    fn process_fixture() {
        let root = PathBuf::from(std::env::var_os("SC_JOB_FIXTURE_ROOT").unwrap());
        let role = std::env::var("SC_JOB_FIXTURE_ROLE").unwrap();
        if role == "grandchild" {
            std::fs::write(root.join("ready"), "grandchild running").unwrap();
            std::thread::sleep(Duration::from_secs(30));
            return;
        }
        let mut child = fixture(&root, "grandchild").spawn().unwrap();
        std::fs::write(root.join("pid"), child.id().to_string()).unwrap();
        std::thread::spawn(move || child.wait().unwrap());
        let deadline = Instant::now() + Duration::from_secs(10);
        while !root.join("ready").exists() {
            assert!(Instant::now() < deadline, "grandchild did not start");
            std::thread::sleep(Duration::from_millis(10));
        }
        eprintln!("grandchild-ready");
        if role == "timeout" {
            std::thread::sleep(Duration::from_secs(30));
        }
    }

    fn scratch() -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "sc-job-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        root
    }

    fn assert_grandchild_stopped(root: &Path) {
        assert_eq!(
            std::fs::read_to_string(root.join("ready")).unwrap(),
            "grandchild running"
        );
        let pid = std::fs::read_to_string(root.join("pid"))
            .unwrap()
            .parse()
            .unwrap();
        // SAFETY: OpenProcess validates the PID and returns an owned handle or null.
        let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_TERMINATE, 0, pid) };
        let stopped = if raw.is_null() {
            assert_eq!(
                io::Error::last_os_error().raw_os_error(),
                Some(ERROR_INVALID_PARAMETER as i32)
            );
            true
        } else {
            let process = owned(raw).unwrap();
            // SAFETY: The process handle grants synchronization rights.
            let stopped =
                unsafe { WaitForSingleObject(process.as_raw_handle(), 1000) } == WAIT_OBJECT_0;
            if !stopped {
                // SAFETY: Clean up a surviving fixture before reporting the failed assertion.
                unsafe {
                    TerminateProcess(process.as_raw_handle(), 1);
                    WaitForSingleObject(process.as_raw_handle(), 5000);
                }
            }
            stopped
        };
        std::fs::remove_dir_all(root).unwrap();
        assert!(stopped, "grandchild survived process-tree cleanup");
    }

    #[test]
    fn timeout_kills_a_running_grandchild_and_captures_stderr() {
        let root = scratch();
        let mut cmd = fixture(&root, "timeout");
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let started = Instant::now();
        let result = run_cmd(&mut cmd, Duration::from_secs(5));
        assert_grandchild_stopped(&root);
        let CommandError::Timeout { stderr, .. } = result.unwrap_err() else {
            panic!("expected a measured timeout");
        };
        assert!(stderr.contains("grandchild-ready"), "{stderr}");
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn finished_parent_does_not_leave_a_grandchild_holding_output_open() {
        let root = scratch();
        let mut cmd = fixture(&root, "success");
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let started = Instant::now();
        let result = run_cmd(&mut cmd, Duration::from_secs(15));
        assert_grandchild_stopped(&root);
        let captured = result.unwrap();
        assert!(captured.status.success(), "{}", captured.stderr);
        assert!(captured.stderr.contains("grandchild-ready"));
        assert!(started.elapsed() < Duration::from_secs(15));
    }
}
