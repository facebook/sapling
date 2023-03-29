/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Cow;
use std::env;
use std::fmt;
use std::io;
use std::io::BufRead;
use std::io::Read;
use std::io::Write;
use std::sync::Mutex;

use anyhow::Context;
use filedescriptor::FileDescriptor;
use filedescriptor::FromRawFileDescriptor;
use filedescriptor::RawFileDescriptor;
use serde::de::DeserializeOwned;
use serde::Serialize;

// 0, 1, 2, ..., file descriptor used by libc (or msvcrt, ucrt).
//
// This is different from RawFileDescriptor, which is
// RawHandle on Windows, and requires conversion using
// `libc::get_osfhandle`.
type LibcFd = libc::c_int;

/// State needed to communicate with nodejs `child_process` IPC.
/// Search `'ipc'` in https://nodejs.org/api/child_process.html
/// for details.
///
/// Under the hood, the IPC uses a single duplex file descriptor
/// for communication. Messages are in new line delimited JSON by
/// default.
pub struct NodeIpc {
    // Mutex is used so the static singleton is easier to use
    // (send and recv do not take &mut self).
    w: Mutex<FileDescriptor>,
    r: Mutex<io::BufReader<FileDescriptor>>,
    // Whether compatible with libuv.
    // If true, on Windows, we'll add extra frame headers per message.
    libuv_compat: bool,
}

impl NodeIpc {
    /// Optionally construct `NodeIpc` from the environment variables.
    /// The environment variables are set by nodejs when it spawns
    /// a child process with `ipc` in `stdio` option.
    ///
    /// On success, return the `NodeIpc` and removes the related environment
    /// variables so they don't leak to child processes accidentally.
    ///
    /// Returns `None` if the environment variables are not set, or if there
    /// are errors initializing internal states.
    pub fn from_env() -> Option<Self> {
        let fd_str = env::var_os("NODE_CHANNEL_FD")?;

        let serialization_mode = env::var_os("NODE_CHANNEL_SERIALIZATION_MODE");
        if let Some(mode) = serialization_mode.as_ref() {
            if mode != "json" {
                // Only JSON serialization is supported.
                return None;
            }
        }

        let raw_fd: LibcFd = fd_str.to_str()?.parse().ok()?;
        let ipc = Self::from_raw_fd(raw_fd).ok()?.with_libuv_compat();

        env::remove_var("NODE_CHANNEL_FD");
        if serialization_mode.is_some() {
            env::remove_var("NODE_CHANNEL_SERIALIZATION_MODE");
        }

        Some(ipc)
    }

    /// Initialize `NodeIpc` from a file descriptor directly.
    /// This is lower level than `from_env` and might be useful
    /// for non-nodejs use-cases. For example, setting up IPC
    /// channel with other processes for talking about other
    /// things.
    pub fn from_raw_fd(raw_fd: LibcFd) -> anyhow::Result<Self> {
        let os_raw_fd: RawFileDescriptor = libc_fd_to_raw_filedescriptor(raw_fd)?;
        let get_fd = || unsafe { FileDescriptor::from_raw_file_descriptor(os_raw_fd) };
        let mut fd = get_fd();
        // On Windows, fd is already non-blocking.
        if cfg!(unix) {
            fd.set_non_blocking(false).with_context(|| {
                format!("in NodeIpc::from_fd, when setting non_blocking on fd {raw_fd:?}")
            })?;
        }

        let r = Mutex::new(io::BufReader::new(fd));
        let w = Mutex::new(get_fd());
        let libuv_compat = false;
        let ipc = Self { r, w, libuv_compat };
        Ok(ipc)
    }

    /// Enable libuv pipe compatibility.
    /// On Windows, frame headers are added per message.
    pub fn with_libuv_compat(mut self) -> Self {
        self.libuv_compat = true;
        self
    }

    /// Send a message to the other side. Might block if the OS buffer is full
    /// and the other side is not receiving the message.
    pub fn send(&self, message: impl Serialize) -> anyhow::Result<()> {
        let mut line = serde_json::to_string(&message)
            .context("in NodeIpc::send, when coverting message to JSON")?;
        line.push('\n');
        self.send_line(line)
    }

    /// Receive a message sent by the other side. Block if there are no new
    /// messages. Returns `None` if the other side has closed the channel.
    pub fn recv<V: DeserializeOwned>(&self) -> anyhow::Result<Option<V>> {
        let line = match self
            .recv_line()
            .context("in NodeIpc::recv, when reading line from file descriptor")?
        {
            None => return Ok(None),
            Some(line) => line,
        };
        let result = serde_json::from_str(&line).with_context(|| {
            format!(
                "in NodeIpc::recv, when deserializing {} to {}",
                FmtString(line.trim_end()),
                std::any::type_name::<V>(),
            )
        })?;
        Ok(Some(result))
    }

    /// Send a line. Blocking. The line should include the ending '\n'.
    #[inline(never)]
    fn send_line(&self, line: String) -> anyhow::Result<()> {
        let mut w = self.w.lock().unwrap();

        let payload = if cfg!(windows) && self.libuv_compat {
            // Emulate libuv pipe frame header on Windows.
            // See https://github.com/libuv/libuv/blob/e1143f12657444c750e47ab3e1fb70ae6a030620/src/win/pipe.c#L1745-L1752
            let mut header = UvPipeWin32FrameHeader::default();
            let len = line.len();
            if len > 0 {
                const UV__IPC_FRAME_HAS_DATA: u32 = 1;
                header.flags |= UV__IPC_FRAME_HAS_DATA;
                header.data_length = len as u32;
            }
            let header: [u8; std::mem::size_of::<UvPipeWin32FrameHeader>()] =
                unsafe { std::mem::transmute(header) };
            let mut payload = Vec::with_capacity(header.len() + len);
            payload.extend_from_slice(&header);
            payload.extend_from_slice(line.as_bytes());
            Cow::Owned(payload)
        } else {
            Cow::Borrowed(line.as_bytes())
        };

        w.write_all(payload.as_ref()).with_context(|| {
            format!(
                "in NodeIpc::send, when sending message {}",
                FmtString(line.trim_end())
            )
        })
    }

    /// Receive a line. Blocking. The line would include the ending '\n'.
    #[inline(never)]
    fn recv_line(&self) -> anyhow::Result<Option<String>> {
        let mut r = self.r.lock().unwrap();
        if cfg!(windows) && self.libuv_compat {
            // libuv adds frame headers on Windows. Skip them.
            let mut libuv_pipe_frame_header = [0u8; std::mem::size_of::<UvPipeWin32FrameHeader>()];
            r.read_exact(&mut libuv_pipe_frame_header)
                .context("in NodeIpc::recv, when reading frame header")?;
        }
        let mut line = String::new();
        let n = r.read_line(&mut line).context("in NodeIpc::recv")?;
        if n == 0 { Ok(None) } else { Ok(Some(line)) }
    }
}

fn libc_fd_to_raw_filedescriptor(fd: LibcFd) -> anyhow::Result<RawFileDescriptor> {
    #[cfg(windows)]
    {
        let handle = unsafe { libc::get_osfhandle(fd) };
        // -1: INVALID_HANDLE; -2: Not associated.
        anyhow::ensure!(
            handle != -1 && handle != -2,
            "libc fd {fd} is invalid ({handle})"
        );
        return Ok(handle as _);
    }

    #[cfg(unix)]
    {
        return Ok(fd);
    }

    #[allow(unreachable_code)]
    {
        unreachable!("unsupported platform");
    }
}

/// Adaptive format of a potentially long string.
struct FmtString<'a>(&'a str);

impl<'a> fmt::Display for FmtString<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let s = self.0;
        let len = s.len();
        if len == 0 {
            write!(f, "<an empty string>")
        } else if len > 128 {
            write!(f, "<string with {len} bytes>")
        } else if s.as_bytes().iter().any(|&b| b == 0) {
            self.0.fmt(f)
        } else {
            write!(f, "<string {:?}>", s.as_bytes())
        }
    }
}

// See https://github.com/libuv/libuv/blob/e1143f12657444c750e47ab3e1fb70ae6a030620/src/win/pipe.c#L74-L79
#[repr(C)]
#[derive(Default)]
struct UvPipeWin32FrameHeader {
    flags: u32,
    reversed1: u32,   // Ignored
    data_length: u32, // Must be zero if there is no data
    reserved2: u32,   // Must be zero
}
