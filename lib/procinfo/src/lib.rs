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

    struct Snapshot {
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

        fn new() -> Result<Snapshot, Error> {
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

        fn get_parent_process_id(&self, process_id: DWORD) -> Result<DWORD, Error> {
            self.find_by_pid(process_id)
                .map(|pe32| pe32.th32ParentProcessID)
        }

        pub fn get_process_executable_name(&self, process_id: DWORD) -> Result<String, Error> {
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

    /// Given a child pid, returns the parent process name.
    pub fn get_parent_process_name() -> Result<String, Error> {
        let mut pid = unsafe { GetCurrentProcessId() };
        let snapshot = Snapshot::new()?;
        loop {
            pid = snapshot.get_parent_process_id(pid)?;
            let name = snapshot.get_process_executable_name(pid)?;
            if !name.contains("hg.") {
                return Ok(name);
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use failure::Error;
    use std::fs::read;

    /// Given a process pid, returns the process name.
    pub fn get_process_name(ppid: i32) -> Result<String, Error> {
        let contents = read(format!("/proc/{}/cmdline", ppid))?;
        let arg0: Vec<u8> = contents.iter().take_while(|&&b| b != 0).cloned().collect();
        let s = String::from_utf8_lossy(arg0.as_slice());
        return Ok(s.trim().to_string());
    }

}

#[cfg(target_os = "macos")]
mod macos {
    use failure::Error;
    use std::process::Command;

    /// Given a process pid, returns the process name.
    pub fn get_process_name(ppid: i32) -> Result<String, Error> {
        // ucomm is the "accounting" name and excludes cruft
        // The D-wrapper saves the arguments which has the advantage
        // of knowing whether node or Java are running Nuclide, Buck,
        // etc., but at the disadvantage of storing a lot of extra
        // space in Scuba.
        let out = Command::new("ps")
            .arg("-o ucomm=")
            .arg("-p")
            .arg(format!("{}", ppid))
            .output()?;
        return Ok(String::from_utf8_lossy(&out.stdout).trim().into());
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

#[cfg(target_os = "linux")]
use self::linux::get_process_name;
#[cfg(target_os = "macos")]
use self::macos::get_process_name;
#[cfg(windows)]
use self::windows::get_parent_process_name;
