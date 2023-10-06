/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Cross-platform utilities for process related logic, like waiting or killing.

use std::io;
use std::time::Duration;
use std::time::Instant;

/// Check if a process exists.
pub fn is_pid_alive(pid: u32) -> bool {
    #[cfg(windows)]
    unsafe {
        win32::is_pid_alive(pid)
    }

    #[cfg(unix)]
    unsafe {
        libc::kill(pid as _, 0) == 0
    }

    #[cfg(all(not(unix), not(windows)))]
    false
}

/// Waits for a process with an optional timeout. Blocks the current thread.
/// Returns `false` if timed out, `true` if the process no longer exists.
pub fn wait_pid(pid: u32, timeout: Option<Duration>) -> bool {
    tracing::debug!("start waiting for pid {}", pid);
    let deadline = timeout.map(|d| Instant::now() + d);
    while is_pid_alive(pid) {
        if let Some(deadline) = deadline.as_ref() {
            if &Instant::now() >= deadline {
                tracing::debug!("timeout waiting for pid {}", pid);
                return false;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    tracing::debug!("complete waiting for pid {}", pid);
    true
}

/// Terminates a process.
///
/// On Windows, this will first send a Ctrl-C event. If the process does not
/// exit in time, try to terminate it.
///
/// On POSIX, this will first send SIGINT, then SIGKILL.
pub fn terminate_pid(pid: u32, grace_period: Option<Duration>) -> io::Result<()> {
    let grace_period = grace_period.unwrap_or_else(|| Duration::from_secs(2));

    #[cfg(windows)]
    unsafe {
        win32::terminate_pid(pid, grace_period)
    }

    #[cfg(unix)]
    unsafe {
        tracing::debug!("sending SIGINT to pid {}", pid);
        let ret = libc::kill(pid as _, libc::SIGINT);
        if ret != 0 {
            return Err(io::Error::last_os_error());
        }
        if !wait_pid(pid, Some(grace_period)) {
            tracing::debug!("sending SIGKILL to pid {}", pid);
            let ret = libc::kill(pid as _, libc::SIGKILL);
            if ret != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    #[cfg(not(any(windows, unix)))]
    {
        let _ = grace_period;
        Ok(())
    }
}

#[cfg(windows)]
mod win32 {
    use std::io;
    use std::mem;
    use std::ptr;
    use std::ptr::null_mut;
    use std::time::Duration;

    use winapi::shared::ntdef::HANDLE;
    use winapi::shared::winerror::ERROR_ACCESS_DENIED;
    use winapi::um::errhandlingapi::GetLastError;
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::jobapi2::AssignProcessToJobObject;
    use winapi::um::jobapi2::CreateJobObjectW;
    use winapi::um::jobapi2::SetInformationJobObject;
    use winapi::um::jobapi2::TerminateJobObject;
    use winapi::um::processthreadsapi::OpenProcess;
    use winapi::um::processthreadsapi::TerminateProcess;
    use winapi::um::wincon::GenerateConsoleCtrlEvent;
    use winapi::um::wincon::CTRL_C_EVENT;
    use winapi::um::winnt::JobObjectExtendedLimitInformation;
    use winapi::um::winnt::JOBOBJECT_EXTENDED_LIMIT_INFORMATION;
    use winapi::um::winnt::JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    use winapi::um::winnt::PROCESS_QUERY_LIMITED_INFORMATION;
    use winapi::um::winnt::PROCESS_SET_QUOTA;
    use winapi::um::winnt::PROCESS_TERMINATE;

    pub(crate) unsafe fn is_pid_alive(pid: u32) -> bool {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle == null_mut() {
            let err = GetLastError();
            if err == ERROR_ACCESS_DENIED {
                // The process exists.
                return true;
            }
            return false;
        }
        CloseHandle(handle);
        return true;
    }

    pub(crate) unsafe fn terminate_pid(pid: u32, grace_period: Duration) -> io::Result<()> {
        tracing::debug!("sending Ctrl+C to pid {}", pid);
        GenerateConsoleCtrlEvent(CTRL_C_EVENT, pid);
        if !crate::wait_pid(pid, Some(grace_period)) {
            tracing::debug!("terminating pid {}", pid);
            let process_handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if process_handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let exit_code = 137; // Matches SIGKILL on unix.
            let ret = TerminateProcess(process_handle, exit_code);
            CloseHandle(process_handle);
            if ret == 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    /// A process group for process tree killing.
    /// This is only available on Windows, backed by a "Job Object".
    pub struct ProcessGroup {
        job_handle: SendHandle,
    }

    struct SendHandle(HANDLE);
    unsafe impl Send for SendHandle {}
    unsafe impl Sync for SendHandle {}

    impl ProcessGroup {
        /// Create a process group.
        pub fn new() -> io::Result<Self> {
            unsafe {
                let job_handle = CreateJobObjectW(null_mut(), null_mut());
                if job_handle.is_null() {
                    return Err(io::Error::last_os_error());
                }
                Ok(Self {
                    job_handle: SendHandle(job_handle),
                })
            }
        }

        /// Add a process to the group.
        /// By default, processes spawned by the process will also be in the group.
        pub fn add(&self, pid: u32) -> io::Result<()> {
            unsafe {
                let process_handle = OpenProcess(PROCESS_TERMINATE | PROCESS_SET_QUOTA, 0, pid);
                if process_handle.is_null() {
                    return Err(io::Error::last_os_error());
                }

                let ret = AssignProcessToJobObject(self.job_handle.0, process_handle);
                CloseHandle(process_handle);

                if ret == 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(())
                }
            }
        }

        /// Terminates all process trees in the group.
        pub fn terminate(&self) -> io::Result<()> {
            unsafe {
                let exit_code = 137; // Similar to exit code caused by SIGKILL.
                let ret = TerminateJobObject(self.job_handle.0, exit_code);
                if ret == 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(())
                }
            }
        }

        /// Terminate all processes automatically at exit, or drop.
        /// This works even when the process is terminated and no ctrlc handlers
        /// have a chance to run.
        pub fn terminate_on_close(&self) -> io::Result<()> {
            unsafe {
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ret = SetInformationJobObject(
                    self.job_handle.0,
                    JobObjectExtendedLimitInformation,
                    ptr::addr_of_mut!(info) as *mut _,
                    mem::size_of_val(&info) as u32,
                );
                if ret == 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(())
                }
            }
        }
    }

    impl Drop for ProcessGroup {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.job_handle.0);
            }
        }
    }
}

#[cfg(windows)]
pub use win32::ProcessGroup;

/// Terminate a process tree on exit.
///
/// This only works on Windows, and works when the current process gets
/// killed in any way (ex. no chance to run atexit or ctrlc handlers).
pub fn terminate_pid_tree_on_exit(pid: u32) -> io::Result<()> {
    #[cfg(windows)]
    {
        // not std OncCell: get_or_try_init is still nightly.
        // https://github.com/rust-lang/rust/issues/109737
        use once_cell::sync::OnceCell;
        static GROUP: OnceCell<ProcessGroup> = OnceCell::new();
        let group = GROUP.get_or_try_init(|| -> io::Result<ProcessGroup> {
            let g = ProcessGroup::new()?;
            g.terminate_on_close()?;
            Ok(g)
        })?;
        group.add(pid)?;
        return Ok(());
    }

    #[cfg(not(windows))]
    {
        let _ = pid;
        Err(io::ErrorKind::Unsupported.into())
    }
}
