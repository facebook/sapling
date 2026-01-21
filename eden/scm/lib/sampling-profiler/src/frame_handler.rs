/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs;
use std::io::Read as _;
use std::os::fd::FromRawFd;

use backtrace_ext::Frame;

use crate::osutil::OwnedFd;

/// Function to process backtraces.
pub type ResolvedBacktraceProcessFunc = Box<dyn Fn(&[String]) + Send + Sync + 'static>;

/// Wraps `Frame` so
#[repr(C)]
#[derive(Clone)]
pub enum MaybeFrame<'a> {
    /// A frame is present.
    Present(Frame<'a>),
    /// No more frames for this backtrace.
    EndOfBacktrace,
}

/// Read, "deserialize" frames from the pipe written by the signal handler.
/// Resolve symbols. Assemble frames into a "backtrace" and hand it over to the
/// specific `process_func`.
///
/// This function is intended to run in a separate thread.
pub fn frame_reader_loop(read_fd: OwnedFd, process_func: ResolvedBacktraceProcessFunc) {
    let mut read_file = match read_fd.into_raw_fd() {
        Some(fd) => unsafe { fs::File::from_raw_fd(fd) },
        None => return,
    };
    let mut frames = Vec::new();
    'main_loop: loop {
        let mut buf: [u8; std::mem::size_of::<MaybeFrame>()] = [0; _];
        if read_file.read_exact(&mut buf).is_err() {
            // The pipe might be closed. `read_file` will be closed on drop.
            break 'main_loop;
        }
        let frame: MaybeFrame = unsafe { std::mem::transmute(buf) };
        match frame {
            MaybeFrame::Present(mut frame) => {
                let name = frame.resolve();
                frames.push(name);
            }
            MaybeFrame::EndOfBacktrace => {
                // "None" means the end of the backtrace.
                process_func(&frames);
                frames.clear();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_size() {
        // See `man pipe2`. We use `O_DIRECT` for "packet-mode" pipes.
        // The packet has size limit: `PIPE_BUF`. The payload (MaybeFrame)
        // must fit in.
        assert!(std::mem::size_of::<MaybeFrame>() <= libc::PIPE_BUF);
    }
}
