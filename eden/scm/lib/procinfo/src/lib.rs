/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn darwin_ppid(pid: u32) -> u32;
    fn darwin_exepath(pid: u32) -> *const libc::c_char;
}

#[cfg(windows)]
mod windows {
    use std::ffi::OsString;
    use std::mem::size_of;
    use std::mem::zeroed;
    use std::os::windows::ffi::OsStringExt;

    use ntapi::ntpsapi::NtQueryInformationProcess;
    use ntapi::ntpsapi::PROCESS_BASIC_INFORMATION;
    use ntapi::ntpsapi::ProcessBasicInformation;
    use winapi::shared::minwindef::DWORD;
    use winapi::shared::minwindef::PULONG;
    use winapi::shared::ntdef::ULONG;
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::processthreadsapi::OpenProcess;
    use winapi::um::psapi::GetProcessImageFileNameW;
    use winapi::um::winnt::HANDLE;
    use winapi::um::winnt::PROCESS_QUERY_INFORMATION;

    pub(crate) fn exe_name(process_id: DWORD) -> Result<String, ()> {
        let process_handle = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION, 0, process_id as _) };
        if process_handle.is_null() {
            return Err(());
        }

        let mut buffer: Vec<u16> = vec![0; 4096];
        let path_len = unsafe {
            GetProcessImageFileNameW(process_handle, buffer.as_mut_ptr(), buffer.len() as u32)
        };

        unsafe { CloseHandle(process_handle) };

        if path_len == 0 {
            return Err(());
        }

        let path = OsString::from_wide(&buffer[..path_len as usize]);
        let name = path
            .to_str()
            .map(|s| s.rsplit(&['\\', '/']).next().unwrap_or("").to_string());
        name.ok_or(())
    }

    pub(crate) fn parent_pid(process_id: DWORD) -> Result<DWORD, ()> {
        let process_handle = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION, 0, process_id as _) };
        if process_handle.is_null() {
            return Err(());
        }

        let mut pbi: PROCESS_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        let mut return_length: ULONG = 0;

        let status = unsafe {
            NtQueryInformationProcess(
                process_handle,
                ProcessBasicInformation,
                &mut pbi as *mut _ as *mut _,
                std::mem::size_of::<PROCESS_BASIC_INFORMATION>() as u32,
                &mut return_length as PULONG,
            )
        };

        unsafe { CloseHandle(process_handle) };

        if status < 0 {
            return Err(());
        }

        Ok(pbi.InheritedFromUniqueProcessId as u32)
    }
}

/// Get the max RSS usage of the current process in bytes.
/// Return 0 on unsupported platform.
pub fn max_rss_bytes() -> u64 {
    #[cfg(unix)]
    {
        let usage = unsafe {
            let mut usage: libc::rusage = std::mem::zeroed();
            libc::getrusage(libc::RUSAGE_SELF, &mut usage as *mut _);
            usage
        };
        // POSIX didn't specify unit of ru_maxrss. Linux uses KB while BSD and
        // OSX use bytes (despite their manpages might say differently).
        let scale = if cfg!(target_os = "linux") {
            1024
        } else {
            // Assume BSD-ish
            1
        };
        return usage.ru_maxrss as u64 * scale;
    }

    #[cfg(windows)]
    {
        use winapi::shared::minwindef::DWORD;
        use winapi::um::processthreadsapi::GetCurrentProcess;
        use winapi::um::psapi::K32GetProcessMemoryInfo;
        use winapi::um::psapi::PROCESS_MEMORY_COUNTERS;
        let mut pmc: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
        pmc.cb = std::mem::size_of_val(&pmc) as DWORD;
        return match unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc, pmc.cb) } {
            0 => 0,
            _ => pmc.PeakWorkingSetSize as u64,
        };
    }

    #[allow(unreachable_code)]
    0
}

// Get current process id.
pub fn current_pid() -> u32 {
    unsafe { libc::getpid() as u32 }
}

/// Get the parent pid. Return 0 on error or unsupported platform.
/// If pid is 0, return the parent pid of the current process.
pub fn parent_pid(pid: u32) -> u32 {
    if pid == 0 {
        #[cfg(unix)]
        return unsafe { libc::getppid() as u32 };

        #[cfg(not(unix))]
        return parent_pid(unsafe { libc::getpid() as u32 });
    }

    #[cfg(target_os = "macos")]
    unsafe {
        return crate::darwin_ppid(pid);
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string(format!("/proc/{}/status", pid)) {
            let prefix = "PPid:";
            for line in content.lines() {
                if let Some(suffix) = line.strip_prefix(prefix) {
                    if let Ok(ppid) = suffix.trim().parse() {
                        return ppid;
                    }
                }
            }
        }
        return 0;
    }

    #[cfg(windows)]
    {
        if let Ok(ppid) = windows::parent_pid(pid) {
            return ppid;
        }
    }

    #[allow(unreachable_code)]
    0
}

/// Get the executable name of the specified pid. Return an empty string on error.
pub fn exe_name(pid: u32) -> String {
    #[cfg(target_os = "macos")]
    {
        use std::ffi::CStr;
        let slice = unsafe { CStr::from_ptr(crate::darwin_exepath(pid)) };
        return slice.to_string_lossy().into_owned();
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(path) = std::fs::read_link(format!("/proc/{}/exe", pid)) {
            return path.into_os_string().to_string_lossy().into_owned();
        }
    }

    #[cfg(windows)]
    {
        if let Ok(name) = windows::exe_name(pid) {
            return name;
        }
    }

    #[allow(unreachable_code)]
    String::new()
}
