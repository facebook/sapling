/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Write;

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

#[cfg(target_os = "macos")]
pub mod macos {
    use std::ffi::CStr;
    use std::mem::zeroed;
    use std::os::raw::c_void;
    use std::path::Path;
    use std::ptr::null_mut;

    use libc::PROC_PIDLISTFDS;
    use libc::PROX_FDTYPE_VNODE;
    use libc::c_char;
    use libc::c_int;
    use libc::off_t;
    use libc::proc_fdinfo;
    use libc::proc_listallpids;
    use libc::proc_pidfdinfo;
    use libc::proc_pidinfo;
    use libc::vnode_info_path;

    /// Return first pid we find that has `path` open, or 0.
    /// This is very inefficient - don't call frequently.
    pub fn file_path_to_pid(path: &Path) -> u32 {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_owned());
        let Some(path) = path.to_str() else {
            return 0;
        };

        let num_pids = unsafe { proc_listallpids(null_mut(), 0) };
        if num_pids < 0 {
            return 0;
        }

        let mut pids: Vec<c_int> = vec![0; num_pids as usize];

        // First get a list of all pids.
        let num_pids = unsafe {
            proc_listallpids(
                pids.as_mut_ptr() as *mut c_void,
                num_pids * size_of::<c_int>() as c_int,
            )
        };
        if num_pids < 0 {
            return 0;
        }
        let num_pids = num_pids as usize;
        if num_pids > pids.len() {
            return 0;
        }

        pids.truncate(num_pids);

        // Limit to 16k files per-process.
        let mut fd_infos = vec![
            proc_fdinfo {
                proc_fd: 0,
                proc_fdtype: 0
            };
            1 << 14
        ];

        for pid in pids {
            // For each pid, get a list of all open fds.
            let num_bytes = unsafe {
                proc_pidinfo(
                    pid,
                    PROC_PIDLISTFDS,
                    0,
                    fd_infos.as_mut_ptr() as *mut c_void,
                    fd_infos.len() as c_int * size_of::<proc_fdinfo>() as c_int,
                )
            };
            if num_bytes < 0 {
                return 0;
            }

            let num_entries = num_bytes as usize / size_of::<proc_fdinfo>();
            if num_entries > fd_infos.len() {
                return 0;
            }

            for fd_info in &fd_infos[..num_entries] {
                if fd_info.proc_fdtype != PROX_FDTYPE_VNODE as u32 {
                    continue;
                }

                #[repr(C)]
                #[allow(non_camel_case_types)]
                struct proc_fileinfo {
                    fi_openflags: u32,
                    fi_status: u32,
                    fi_offset: off_t,
                    fi_type: i32,
                    rfu_1: i32,
                }

                #[repr(C)]
                #[allow(non_camel_case_types)]
                struct vnode_fdinfowithpath {
                    pfi: proc_fileinfo,
                    pvip: vnode_info_path,
                }

                const PROC_PIDFDVNODEPATHINFO: c_int = 2;

                let mut vnode_info = unsafe { zeroed::<vnode_fdinfowithpath>() };

                // For each fd, translate into file path.
                let num_bytes = unsafe {
                    proc_pidfdinfo(
                        pid,
                        fd_info.proc_fd,
                        PROC_PIDFDVNODEPATHINFO,
                        &mut vnode_info as *mut _ as *mut c_void,
                        size_of_val(&vnode_info) as c_int,
                    )
                };
                if num_bytes < size_of_val(&vnode_info) as c_int {
                    continue;
                }

                let buf_len = size_of_val(&vnode_info.pvip.vip_path);

                // vip_path is [[c_char; 32]; 32] - flatten it out.
                let path_buf = &mut vnode_info.pvip.vip_path as *mut _ as *mut c_char;

                // Make sure we are null terminated.
                unsafe { *path_buf.add(buf_len - 1) = 0 }

                let c_str: &CStr = unsafe { CStr::from_ptr(path_buf) };

                if c_str.to_bytes() == path.as_bytes() {
                    return pid as u32;
                }
            }
        }

        0
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

/// Get a description of pid's ancestors, including pid itself.
pub fn ancestors(mut pid: u32) -> String {
    if pid == 0 {
        pid = current_pid();
    }

    let mut buf = String::new();

    let mut count = 0;
    while pid > 0 {
        count += 1;
        if count >= 16 {
            let _ = write!(&mut buf, "...");
            break;
        }

        if !buf.is_empty() {
            let _ = write!(&mut buf, " <- ");
        }

        let name = exe_name(pid);

        // Trim to last part of path to keep compact.
        let name = name
            .rsplit(if cfg!(windows) {
                &['/', '\\'][..]
            } else {
                &['/'][..]
            })
            .next()
            .unwrap_or(&name);

        if name.is_empty() {
            let _ = write!(&mut buf, "{pid}");
        } else {
            let _ = write!(&mut buf, "{name}({pid})");
        }

        pid = parent_pid(pid);
    }

    buf
}

#[cfg(test)]
mod test {
    #[cfg(target_os = "macos")]
    #[test]
    fn test_file_name_to_pid() {
        let dir = tempfile::TempDir::new().unwrap();
        let file_path = dir.path().join("file");

        let _file = std::fs::File::create(&file_path).unwrap();

        assert_eq!(
            super::macos::file_path_to_pid(&file_path),
            std::process::id()
        );

        drop(_file);
    }
}
