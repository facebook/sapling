/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use filedescriptor::RawFileDescriptor;
use serde::Deserialize;
use serde::Serialize;

use crate::nodeipc::NodeIpc;

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

        #[allow(unreachable_code)]
        {
            anyhow::bail!("platform is not supported for sending file descriptors.");
        }
    }

    /// The other end of `send_fd_vec`. Return `SendFdPayload` with `raw_fds`
    /// containing the received fds.
    pub fn recv_fd_vec(&self) -> anyhow::Result<SendFdPayload> {
        self.check_sendfd_compatibility()?;

        #[cfg(windows)]
        {
            use winapi::um::handleapi::CloseHandle;
            use winapi::um::handleapi::DuplicateHandle;
            use winapi::um::processthreadsapi::GetCurrentProcess;
            use winapi::um::processthreadsapi::OpenProcess;
            use winapi::um::wincon::AttachConsole;
            use winapi::um::wincon::FreeConsole;
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

            if payload.raw_fds.iter().any(|h| h.is_null()) {
                unsafe {
                    FreeConsole();
                    AttachConsole(payload.pid)
                };
            }

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

        #[allow(unreachable_code)]
        {
            anyhow::bail!("platform is not supported for receiving file descriptors.");
        }
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
