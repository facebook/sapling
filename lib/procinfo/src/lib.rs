// Copyright 2004-present Facebook. All Rights Reserved.

#[cfg(target_os = "macos")]
extern "C" {
    fn darwin_ppid(pid: u32) -> u32;
    fn darwin_exepath(pid: u32) -> *const libc::c_char;
}

#[cfg(windows)]
mod windows {
    use failure::{format_err, Error};
    use kernel32::{
        CloseHandle, CreateToolhelp32Snapshot, GetCurrentProcess, GetCurrentProcessId, GetFileType,
        GetLastError, GetStdHandle, K32GetProcessMemoryInfo, Process32FirstW, Process32NextW,
    };
    use std::ffi::OsString;
    use std::mem::{size_of, size_of_val, zeroed};
    use std::os::windows::ffi::OsStringExt;
    use winapi::psapi::PROCESS_MEMORY_COUNTERS;
    use winapi::winbase::{FILE_TYPE_CHAR, STD_OUTPUT_HANDLE};
    use winapi::{DWORD, HANDLE, INVALID_HANDLE_VALUE, PROCESSENTRY32W, TH32CS_SNAPPROCESS};

    pub(crate) struct Snapshot {
        handle: HANDLE,
    }

    impl Drop for Snapshot {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.handle) };
        }
    }

    impl Snapshot {
        fn new_pe32() -> PROCESSENTRY32W {
            let mut pe: PROCESSENTRY32W = unsafe { zeroed() };
            pe.dwSize = size_of::<PROCESSENTRY32W>() as DWORD;
            pe
        }

        pub(crate) fn new() -> Result<Snapshot, Error> {
            let snapshot_handle: HANDLE =
                unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };

            if snapshot_handle == INVALID_HANDLE_VALUE {
                return Err(format_err!(
                    "failed to create the ToolHelp snapshot: {:?}",
                    unsafe { GetLastError() }
                ));
            }

            Ok(Snapshot {
                handle: snapshot_handle,
            })
        }

        fn find<F>(&self, condition: F) -> Result<PROCESSENTRY32W, Error>
        where
            F: Fn(&PROCESSENTRY32W) -> bool,
        {
            let mut pe32 = Snapshot::new_pe32();
            if unsafe { Process32FirstW(self.handle, &mut pe32) } == 0 {
                return Err(format_err!(
                    "failed to call Process32FirstW: {:?}",
                    unsafe { GetLastError() }
                ));
            }

            loop {
                if condition(&pe32) {
                    return Ok(pe32);
                }

                if unsafe { Process32NextW(self.handle, &mut pe32) } == 0 {
                    break;
                }
            }

            Err(format_err!(
                "could not find a process matching a condition: {:?}",
                unsafe { GetLastError() }
            ))
        }

        fn find_by_pid(&self, pid: DWORD) -> Result<PROCESSENTRY32W, Error> {
            self.find(|pe32| pe32.th32ProcessID == pid)
        }

        pub(crate) fn get_parent_process_id(&self, process_id: DWORD) -> Result<DWORD, Error> {
            self.find_by_pid(process_id)
                .map(|pe32| pe32.th32ParentProcessID)
        }

        pub(crate) fn get_process_executable_name(
            &self,
            process_id: DWORD,
        ) -> Result<String, Error> {
            self.find_by_pid(process_id)
                .map(|pe32| pe32.szExeFile)
                .map(|cs| {
                    cs.into_iter()
                        .take_while(|&&i| i != 0)
                        .map(|&i| i as u16)
                        .collect::<Vec<u16>>()
                })
                .map(|ref v| OsString::from_wide(v).into_string().unwrap_or("".into()))
        }
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
        use kernel32::{GetCurrentProcess, K32GetProcessMemoryInfo};
        use winapi::psapi::PROCESS_MEMORY_COUNTERS;
        type DWORD = u32;
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
                if line.starts_with(prefix) {
                    if let Ok(ppid) = line[prefix.len()..].trim().parse() {
                        return ppid;
                    }
                }
            }
        }
        return 0;
    }

    #[cfg(windows)]
    {
        if let Ok(snapshot) = windows::Snapshot::new() {
            if let Ok(ppid) = snapshot.get_parent_process_id(pid) {
                return ppid;
            }
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
        if let Ok(snapshot) = windows::Snapshot::new() {
            if let Ok(name) = snapshot.get_process_executable_name(pid) {
                return name;
            }
        }
    }

    #[allow(unreachable_code)]
    String::new()
}
