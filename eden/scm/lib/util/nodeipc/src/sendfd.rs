/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Context;
use filedescriptor::AsRawFileDescriptor;
use filedescriptor::RawFileDescriptor;
use serde::Deserialize;
use serde::Serialize;

use crate::nodeipc::LibcFd;
use crate::nodeipc::NodeIpc;
use crate::singleton::IPC;

impl NodeIpc {
    /// Send a list of fd (or HANDLE on Windows).
    /// The other side can use `recv_fd_vec` to receive them.
    ///
    /// Note: if the other side is nodejs, it will not understand this special
    /// message. Nodejs has a different implementation. You can use
    /// `subprocess.send(message, sendHandle)` between nodejs processes.
    pub fn send_fd_vec(&self, fds: &[RawFileDescriptor]) -> anyhow::Result<()> {
        self.check_sendfd_compatibility()?;

        #[cfg(windows)]
        {
            use winapi::um::fileapi::GetFileType;
            use winapi::um::winbase::FILE_TYPE_CHAR;
            use winapi::um::winnt::HANDLE;

            let mut sendable_fds = Vec::with_capacity(fds.len());
            for &handle in fds {
                let file_type = unsafe { GetFileType(handle as HANDLE) };
                if file_type == FILE_TYPE_CHAR {
                    sendable_fds.push(std::ptr::null_mut());
                } else {
                    sendable_fds.push(handle);
                }
            }
            let payload = SendFdPayload {
                pid: std::process::id(),
                raw_fds: sendable_fds,
            };
            return self.send(payload);
        }

        #[cfg(unix)]
        {
            use std::mem;

            let fds_byte_size = mem::size_of_val(fds);
            let (mut cmsgs, opaque, hdr) = cmsg_vec_and_msghdr(fds_byte_size);

            let cmsg = &mut cmsgs[0];
            cmsg.cmsg_level = libc::SOL_SOCKET;
            cmsg.cmsg_type = libc::SCM_RIGHTS;
            cmsg.cmsg_len = unsafe { libc::CMSG_LEN(fds_byte_size as u32) } as _;

            // The man page warns that `CMSG_DATA` is not aligned (to `RawFileDescriptor`)
            // and suggests `memcpy`.
            let cmsg_data = unsafe { libc::CMSG_DATA(cmsg) };
            unsafe { libc::memcpy(cmsg_data as *mut _, fds.as_ptr() as *const _, fds_byte_size) };

            let w = self.w.lock().unwrap();
            let socket_fd = w.as_raw_file_descriptor();
            let ret = unsafe { libc::sendmsg(socket_fd, &hdr, 0) };
            if ret < 0 {
                return Err(std::io::Error::last_os_error())
                    .with_context(|| format!("Failed to sendmsg with fds {:?}", &fds));
            }
            drop((cmsgs, opaque));

            return Ok(());
        }

        #[allow(unreachable_code)]
        {
            anyhow::bail!("platform is not supported for sending file descriptors.");
        }
    }

    /// The other end of `send_fd_vec`. Return `SendFdPayload` with `raw_fds`
    /// containing the received fds.
    ///
    /// This cannot be used to receive handles sent via nodejs'
    /// `subprocess.send(message, sendHandle)` API.
    ///
    /// On POSIX systems, at most 32 fds can be received once.
    /// See `MAX_FD_COUNT` below.
    pub fn recv_fd_vec(&self) -> anyhow::Result<SendFdPayload> {
        self.check_sendfd_compatibility()?;

        #[cfg(windows)]
        {
            use winapi::um::handleapi::CloseHandle;
            use winapi::um::handleapi::DuplicateHandle;
            use winapi::um::processthreadsapi::GetCurrentProcess;
            use winapi::um::processthreadsapi::OpenProcess;
            use winapi::um::winnt::DUPLICATE_SAME_ACCESS;
            use winapi::um::winnt::HANDLE;
            use winapi::um::winnt::PROCESS_DUP_HANDLE;

            let mut payload: SendFdPayload = match self.recv::<SendFdPayload>()? {
                Some(payload) => payload,
                None => anyhow::bail!("Unexpected EOF when receiving fd"),
            };
            let mut received_handles = Vec::with_capacity(payload.raw_fds.len());
            let mut process_handle: HANDLE = std::ptr::null_mut();

            struct CloseOnDrop(HANDLE);
            impl Drop for CloseOnDrop {
                fn drop(&mut self) {
                    unsafe { CloseHandle(self.0) };
                }
            }

            let mut close_on_drop = None;

            for source_handle in payload.raw_fds {
                if source_handle.is_null() {
                    received_handles.push(source_handle);
                    continue;
                }
                // Open process for handle duplication.
                if process_handle.is_null() {
                    process_handle = unsafe {
                        OpenProcess(PROCESS_DUP_HANDLE, /* bInheritHandle */ 0, payload.pid)
                    };
                    if process_handle.is_null() {
                        return Err(std::io::Error::last_os_error()).with_context(|| {
                            format!("OpenProcess(pid={}) for DuplicateHandle", payload.pid)
                        });
                    }
                    close_on_drop = Some(CloseOnDrop(process_handle));
                }

                // DuplicateHandle can "steal" a handle from another process.
                let mut dup_handle = std::ptr::null_mut();
                let ret = unsafe {
                    DuplicateHandle(
                        process_handle,
                        source_handle as HANDLE,
                        GetCurrentProcess(),
                        &mut dup_handle,
                        /* dwDesiredAccess */ 0,
                        /* bInheritHandle */ 0,
                        DUPLICATE_SAME_ACCESS,
                    )
                };
                if ret == 0 {
                    return Err(std::io::Error::last_os_error()).with_context(|| {
                        format!(
                            "DuplicateHandle(pid={}, handle={:?})",
                            payload.pid, source_handle
                        )
                    });
                }
                received_handles.push(dup_handle as _);
            }

            // Replace raw_fds. They were in the source process. Now we got `received_handles` in this process.
            payload.raw_fds = received_handles;

            // Shut rustc up about unused variable or assignment.
            drop(close_on_drop);

            return Ok(payload);
        }

        #[cfg(unix)]
        unsafe {
            use std::mem;

            const MAX_FD_COUNT: usize = 32;
            let fds_byte_size = mem::size_of::<RawFileDescriptor>() * MAX_FD_COUNT;
            let (cmsgs, opaque, mut hdr) = cmsg_vec_and_msghdr(fds_byte_size);

            let r = self.r.lock().unwrap();
            assert!(r.buffer().is_empty());
            let socket_fd = r.get_ref().as_raw_file_descriptor();

            let ret = libc::recvmsg(socket_fd, &mut hdr, 0);
            if ret < 0 {
                return Err(std::io::Error::last_os_error()).context("Failed to recvmsg");
            }

            let mut received_fds = Vec::<RawFileDescriptor>::new();
            let mut cmsg = libc::CMSG_FIRSTHDR(&hdr);
            while !cmsg.is_null() {
                if (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS {
                    let data = libc::CMSG_DATA(cmsg);
                    let data_size = (*cmsg).cmsg_len as usize - libc::CMSG_LEN(0) as usize;
                    let mut fds = vec![
                        -1 as RawFileDescriptor;
                        data_size / mem::size_of::<RawFileDescriptor>()
                    ];
                    assert_eq!(fds.len() * mem::size_of::<RawFileDescriptor>(), data_size);
                    // `data` might be not aligned. Use `memcpy` to copy.
                    libc::memcpy(fds.as_mut_ptr() as *mut _, data as *const _, data_size);
                    received_fds.extend(fds);
                }
                cmsg = libc::CMSG_NXTHDR(&hdr, cmsg);
            }
            drop((cmsgs, opaque));

            let payload = SendFdPayload {
                raw_fds: received_fds,
            };

            return Ok(payload);
        }

        #[allow(unreachable_code)]
        {
            anyhow::bail!("platform is not supported for receiving file descriptors.");
        }
    }

    /// Send the stdio and optionally the `NODE_CHANNEL_FD` file descriptor
    /// (the singleton) for the other end to "attach".
    pub fn send_stdio(&self) -> anyhow::Result<()> {
        let mut fds = Vec::<RawFileDescriptor>::with_capacity(4);

        #[cfg(windows)]
        unsafe {
            use winapi::um::processenv::GetStdHandle;

            fds.extend(
                stdio_constants()
                    .iter()
                    .map(|c| GetStdHandle(c.win_constant) as RawFileDescriptor),
            );
        }

        #[cfg(unix)]
        {
            fds.extend(
                stdio_constants()
                    .iter()
                    .map(|c| c.libc_fd as RawFileDescriptor),
            )
        }

        // Optionally, include the singleton file descriptor.
        if let Some(ipc) = crate::get_singleton() {
            if let Ok(w) = ipc.w.lock() {
                fds.push(w.as_raw_file_descriptor());
            }
        }

        self.send_fd_vec(&fds)?;
        Ok(())
    }

    /// Replace the stdio using the one sent from the other end.
    /// Update the singleton to match the sender.
    ///
    /// On Windows, the console might be replaced to the sender's.
    pub fn recv_stdio(&self) -> anyhow::Result<()> {
        let payload = self.recv_fd_vec()?;

        // Replace the stdio.
        #[cfg(unix)]
        {
            for (&received_fd, std_constant) in payload.raw_fds.iter().zip(stdio_constants()) {
                if received_fd > 0 && received_fd != std_constant.libc_fd as RawFileDescriptor {
                    unsafe {
                        libc::dup2(received_fd, std_constant.libc_fd as _);
                        libc::close(received_fd);
                    }
                }
            }
        }

        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;

            use winapi::um::processenv::SetStdHandle;
            use winapi::um::wincon::AttachConsole;
            use winapi::um::wincon::FreeConsole;

            if payload.raw_fds.iter().any(|h| h.is_null()) {
                unsafe {
                    FreeConsole();
                    AttachConsole(payload.pid)
                };
            }

            for (&received_handle, std_constant) in payload.raw_fds.iter().zip(stdio_constants()) {
                let mut std_handle = received_handle;
                if received_handle.is_null() {
                    // This is a "console" handle. In case stdio was redirected to `NULL`, we need
                    // extra steps to restore them:
                    // - Get the console handle by via CreateFile win_name.
                    // - Set the libc fd to the handle.
                    //   Failing to do so affects programs using libc (ex. printf), for example,
                    //   `less.exe` run as a child might not be able to write anything.
                    // - SetStdHandle to update the std handle to the console handle.
                    //   Failing to do means things like `println!` won't work in this process.
                    //   If you run the `spawn_sendfd` example, the child won't print
                    //   "Child: write to stderr".

                    // According to https://learn.microsoft.com/en-us/windows/console/console-handles,
                    // the std handles have GENERIC_READ | GENERIC_WRITE access.
                    let file = std::fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open(std_constant.win_file_name)
                        .with_context(|| format!("Cannot open {}", std_constant.win_file_name))?;
                    let handle = file.as_raw_handle();
                    let std_libc_fd = std_constant.libc_fd;
                    std_handle = handle as _;

                    // File      HANDLE          LibcFd         Note
                    // ---------------------------------------------------------------------
                    // [file] => [handle]     => [new_libc_fd]  fd allocated by libc
                    //                              |
                    //           [new_handle] <- [std_libc_fd]  fd specified by us (0, 1, 2)
                    //
                    // [A] => [B]: Transfer ownership from A to B. B takes care of closing.
                    // [A] <- [B]: Derive A from B. B takes care of closing.
                    // |         : Duplicate. Different rows are backed by different LibcFds.
                    let new_libc_fd = unsafe { libc::open_osfhandle(handle as _, libc::O_RDWR) };
                    if new_libc_fd == -1 {
                        return Err(std::io::Error::last_os_error())
                            .context("Cannot open_osfhandle");
                    }
                    if new_libc_fd >= 0 && new_libc_fd as LibcFd != std_libc_fd {
                        // Replace `std_libc_fd` using `dup2`.
                        let dup_success = unsafe { libc::dup2(new_libc_fd, std_libc_fd as _) } == 0;
                        if !dup_success {
                            return Err(std::io::Error::last_os_error()).context("Cannot dup2");
                        }
                        // We want to close `new_libc_fd`, which will close `handle`.
                        // So we need to get the newly "dup"ed handle first.
                        let new_handle = unsafe { libc::get_osfhandle(std_libc_fd) };
                        if new_handle >= 0 {
                            std_handle = new_handle as _;
                            unsafe { libc::close(new_libc_fd) };
                        }
                    }

                    // The underlying handle of `file` is either owned (and closed) by libc,
                    // or being used by StdHandle. Do not close it.
                    std::mem::forget(file);
                }

                unsafe { SetStdHandle(std_constant.win_constant, std_handle as _) };
            }
        }

        // Replace the singleton.
        let mut ipc = IPC.write().unwrap();
        if let Some(&raw_fd) = payload.raw_fds.get(stdio_constants().len()) {
            let new_ipc = NodeIpc::from_raw_file_descriptor(raw_fd)?.with_libuv_compat();
            *ipc = Some(Some(Arc::new(new_ipc)));
        } else {
            *ipc = Some(None);
        }

        Ok(())
    }

    fn check_sendfd_compatibility(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.libuv_compat,
            "send_fd_vec() and recv_fd_vec() are incompatible with libuv compatibility."
        );
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SendFdPayload {
    #[cfg(windows)]
    /// Sender pid. Useful for `AttachConsole` on Windows.
    pub pid: u32,

    /// Raw handles or fds. Normalized as u64 for serialization.
    /// On Winodws, `null` is a placeholder indicating an absent handle.
    #[serde(with = "serde_raw_fds")]
    pub raw_fds: Vec<RawFileDescriptor>,
}

// Serialize raw fds as u64. Note serde_json can round-trip u64 just fine,
// even if f64 cannot represent the number.
mod serde_raw_fds {
    use filedescriptor::RawFileDescriptor;
    use serde::ser::SerializeSeq;
    use serde::Deserialize;
    use serde::Deserializer;
    use serde::Serializer;

    pub fn serialize<S>(raw_fds: &Vec<RawFileDescriptor>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(raw_fds.len()))?;
        for &fd in raw_fds {
            let item = fd as u64;
            seq.serialize_element(&item)?;
        }
        seq.end()
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<RawFileDescriptor>, D::Error>
    where
        D: Deserializer<'de>,
    {
        // In theory we can implement a serde visitor to avoid allocating a Vec.
        // But it's too verbose.
        let seq: Vec<u64> = Deserialize::deserialize(deserializer)?;
        let raw_fds: Vec<RawFileDescriptor> =
            seq.into_iter().map(|v| v as RawFileDescriptor).collect();
        Ok(raw_fds)
    }
}

/// Create a `cmsg` buffer for `msghdr.msg_control`. Then create a `msghdr` that refers to `cmsg`
/// buffer, with a dummy iov buffer.
///
/// Returns `(cmsgs, opaque, msghdr)`.
/// The callsite needs to keep `cmsgs` and `opaque` alive before dropping `msghdr`,
/// since `msghdr` contains pointers to them.
/// The callsite might want to modify `cmsgs[0]` to customize the control message.
/// Note the `cmsgs` is actually a union with bytes payload, so `cmsgs[1]` should
/// not be used.
#[cfg(unix)]
fn cmsg_vec_and_msghdr(
    byte_size: usize,
) -> (Vec<libc::cmsghdr>, (impl Drop, impl Drop), libc::msghdr) {
    use std::mem;

    // See `man cmsg`.
    let cmsg_space: usize = unsafe { libc::CMSG_SPACE(byte_size as _) } as _;
    let cmsg_vec_len: usize = {
        let cmsghdr_byte_size = mem::size_of::<libc::cmsghdr>();
        (cmsg_space + cmsghdr_byte_size - 1) / cmsghdr_byte_size
    };
    assert!(cmsg_vec_len >= 1);
    let mut cmsg_buf: Vec<libc::cmsghdr> = vec![unsafe { mem::zeroed() }; cmsg_vec_len];

    // See `man sendmsg`. We need a non-empty dummy message to actually send information out.
    let mut iov_buf = vec![b'\n'];
    let mut dummy_iov = Box::new(libc::iovec {
        iov_base: iov_buf.as_mut_ptr() as *mut _,
        iov_len: iov_buf.len(),
    });
    let hdr = libc::msghdr {
        msg_iov: dummy_iov.as_mut(),
        msg_iovlen: 1,
        msg_control: cmsg_buf.as_mut_ptr() as *mut _,
        msg_controllen: (cmsg_buf.len() * mem::size_of_val(&cmsg_buf[0])) as _,
        ..unsafe { mem::zeroed() }
    };

    (cmsg_buf, (iov_buf, dummy_iov), hdr)
}

struct StdioConstant {
    libc_fd: LibcFd,
    // Constant used by SetStdHandle, GetStdHandle.
    #[cfg(windows)]
    win_constant: winapi::shared::minwindef::DWORD,
    // File name to create the console handle.
    // See also https://learn.microsoft.com/en-us/windows/console/console-handles
    #[cfg(windows)]
    win_file_name: &'static str,
}

fn stdio_constants() -> &'static [StdioConstant] {
    #[cfg(windows)]
    use winapi::um::winbase;

    // libc::STDIN_FILENO etc are undefined on Windows.
    // So we just use 0, 1, 2 numbers below.
    // Statically assert the libc constants match 0, 1, 2.
    #[cfg(unix)]
    // We do want the code to be optimized out by the compiler.
    #[allow(clippy::assertions_on_constants)]
    const _: () = {
        assert!(libc::STDIN_FILENO == 0);
        assert!(libc::STDOUT_FILENO == 1);
        assert!(libc::STDERR_FILENO == 2);
    };

    &[
        StdioConstant {
            libc_fd: 0,
            #[cfg(windows)]
            win_constant: winbase::STD_INPUT_HANDLE,
            #[cfg(windows)]
            win_file_name: "CONIN$",
        },
        StdioConstant {
            libc_fd: 1,
            #[cfg(windows)]
            win_constant: winbase::STD_OUTPUT_HANDLE,
            #[cfg(windows)]
            win_file_name: "CONOUT$",
        },
        StdioConstant {
            libc_fd: 2,
            #[cfg(windows)]
            win_constant: winbase::STD_ERROR_HANDLE,
            // There is no CONERR$.
            #[cfg(windows)]
            win_file_name: "CONOUT$",
        },
    ]
}
