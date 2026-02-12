/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs;
use std::io;
use std::io::Read as _;
use std::os::fd::FromRawFd;

use backtrace_ext::Frame;

use crate::ResolvedBacktraceProcessFunc;
use crate::osutil::OwnedFd;

/// `Frame` payload being written to pipes.
#[repr(C)]
#[derive(Clone)]
pub struct FramePayload {
    /// Identity of a backtrace. Used to detect incomplete backtraces.
    pub backtrace_id: usize,
    /// Auto-incremental in a signal backtrace. Used to detect incomplete backtraces.
    pub depth: usize,
    pub frame: MaybeFrame,
}

#[repr(C)]
#[derive(Clone)]
pub enum MaybeFrame {
    /// A frame is present.
    Present(Frame),
    /// No more frames for this backtrace.
    EndOfBacktrace,
}

/// Read, "deserialize" frames from the pipe written by the signal handler.
/// Resolve symbols. Assemble frames into a "backtrace" and hand it over to the
/// specific `process_func`.
///
/// This function is intended to run in a separate thread.
pub fn frame_reader_loop(read_fd: OwnedFd, mut process_func: ResolvedBacktraceProcessFunc) {
    let mut read_file = match read_fd.into_raw_fd() {
        Some(fd) => unsafe { fs::File::from_raw_fd(fd) },
        None => return,
    };
    let mut frames = Vec::new();
    let mut current_backtrace_id = 0;
    let mut expected_depth = 0;
    'main_loop: loop {
        const SIZE: usize = std::mem::size_of::<FramePayload>();
        let mut buf: [u8; SIZE] = [0; _];
        match read_file.read_exact(&mut buf) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                // The pipe was closed. `read_file` will be closed on drop.
                break 'main_loop;
            }
            Err(_) => {
                // Incomplete packet? Ignore.
                continue;
            }
        }
        // safety: FramePayload is `repr(C)` and contains only `usize` fields.
        // It is okay to use `transmute` to "deserialize" within the same process.
        let frame: FramePayload = unsafe { std::mem::transmute(buf) };
        if frame.backtrace_id != current_backtrace_id {
            // A different backtrace (implies a missing EndOfBacktrace).
            frames.clear();
            current_backtrace_id = frame.backtrace_id;
            expected_depth = 0;
        }
        if frame.depth as isize != expected_depth {
            // This backtrace is bad (out of sync).
            // Ignore the rest of the frames of the same backtrace.
            frames.clear();
            expected_depth = -1;
            continue;
        } else {
            expected_depth += 1;
        }
        match frame.frame {
            MaybeFrame::Present(frame) => {
                let name = frame.resolve();
                frames.push(name);
            }
            MaybeFrame::EndOfBacktrace => {
                // The end of a backtrace.
                if !frames.is_empty() {
                    process_func(&frames);
                }
                frames.clear();
                expected_depth = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn test_frame_size() {
        // See `man pipe2`. We use `O_DIRECT` for "packet-mode" pipes.
        // The packet has size limit: `PIPE_BUF`. The payload (MaybeFrame)
        // must fit in.
        assert!(std::mem::size_of::<FramePayload>() <= libc::PIPE_BUF);
    }
}
