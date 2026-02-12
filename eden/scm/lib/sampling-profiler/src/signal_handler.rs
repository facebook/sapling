/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use backtrace_ext::trace_unsynchronized;

use crate::frame_handler::FramePayload;
use crate::frame_handler::MaybeFrame;

/// The signal handler is called every second on the main thread. It should
/// collect minimal stack info while the main logic of the main thread is
/// paused, and pass the info over pipe for further processing.
/// Native function symbolization can be done in other threads. Python function
/// symbolization must be partially done now, since the PyFrame objects might be
/// deallocated soon.
pub extern "C" fn signal_handler(
    sig: libc::c_int,
    info: *const libc::siginfo_t,
    _data: *const libc::c_void,
) {
    if sig != libc::SIGPROF {
        return;
    }

    // On Linux, the payload (write fd) is delivered via sigevent's si_value.
    // On macOS, it's passed via an atomic (no si_value support with pthread_kill).
    #[cfg(target_os = "linux")]
    let write_fd = {
        if info.is_null() {
            return;
        }
        let write_fd: isize = unsafe {
            let sigev = (*info).si_value();
            std::mem::transmute(sigev)
        };
        if write_fd < 0 {
            return;
        }
        write_fd as i32
    };

    #[cfg(all(unix, not(target_os = "linux")))]
    let write_fd = {
        let _ = info;
        let payload = crate::osutil::SIGNAL_PAYLOAD.swap(-1, Ordering::AcqRel);
        if payload < 0 {
            return;
        }
        payload as i32
    };

    // libc::write (and other syscalls) may clobber errno.
    let saved_errno = unsafe { get_errno() };

    let backtrace_id: usize = {
        static BACKTRACE_ID: AtomicUsize = AtomicUsize::new(0);
        BACKTRACE_ID.fetch_add(1, Ordering::AcqRel)
    };
    let mut depth = 0;

    // Skip the first frames.
    const SKIP_FRAMES: usize = if cfg!(target_os = "linux") {
        // - signal_handler (this function)
        // - __sigaction
        2
    } else if cfg!(target_os = "macos") {
        // - backtrace::trace_unsynchronized
        // - signal_handler (this function)
        // - __sigtramp
        3
    } else {
        // Guess
        2
    };
    trace_unsynchronized!(|frame| {
        if depth >= SKIP_FRAMES {
            let maybe_frame = MaybeFrame::Present(frame);
            let payload = FramePayload {
                backtrace_id,
                depth: depth.saturating_sub(SKIP_FRAMES),
                frame: maybe_frame,
            };
            if write_frame(&payload, write_fd) != 0 {
                // Poison `depth` so this "incomplete" backtrace gets dropped.
                depth += 2;
                return false;
            }
        }
        depth += 1;
        true
    });

    // Write a placeholder frame to mark an end of the current backtrace.
    let end_frame = MaybeFrame::EndOfBacktrace;
    let payload = FramePayload {
        backtrace_id,
        depth: depth.saturating_sub(SKIP_FRAMES),
        frame: end_frame,
    };
    let _ = write_frame(&payload, write_fd);

    unsafe { set_errno(saved_errno) };
}

/// Write a `MaybeFrame`. Handles EINTR.
/// Return 0 on success. Return errno otherwise.
///
/// This function is to be called from a signal handler, and intentionally
/// avoids high-level Rust types like `io::Result`. So its easier to audit
/// async-signal-safety.
fn write_frame(frame: &FramePayload, fd: libc::c_int) -> libc::c_int {
    let size = std::mem::size_of::<FramePayload>();
    let mut remaining_bytes = size;
    let mut pos = frame as *const FramePayload as *const libc::c_void;
    loop {
        // safety: FramePayload is `repr(C)` and contains only `usize` fields.
        // It is okay to write its raw bytes to "serialize" within the same process.
        let written_bytes = unsafe { libc::write(fd, pos, remaining_bytes) };
        if written_bytes < 0 {
            let errno = unsafe { get_errno() };
            if errno == libc::EINTR {
                // Retry
                continue;
            } else {
                return errno;
            }
        }
        remaining_bytes = remaining_bytes.saturating_sub(written_bytes as usize);
        if remaining_bytes == 0 {
            return 0;
        } else if cfg!(target_os = "linux") {
            // On Linux, the pipe is in "packet" mode (O_DIRECT).
            // Discard this packet.
            return libc::EINVAL;
        } else if written_bytes == 0 {
            return libc::EINVAL;
        } else {
            // On other unix systems, the pipe is not in packet mode.
            // Continue writing the rest of the data.
            pos = unsafe { pos.offset(written_bytes) };
            continue;
        }
    }
}

/// Read `errno` for the current thread. Async-signal-safe.
unsafe fn get_errno() -> libc::c_int {
    #[cfg(target_os = "macos")]
    unsafe {
        *libc::__error()
    }
    #[cfg(target_os = "linux")]
    unsafe {
        *libc::__errno_location()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        0
    }
}

/// Write `errno` for the current thread. Async-signal-safe.
unsafe fn set_errno(value: libc::c_int) {
    #[cfg(target_os = "macos")]
    unsafe {
        *libc::__error() = value;
    }
    #[cfg(target_os = "linux")]
    unsafe {
        *libc::__errno_location() = value;
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = value;
    }
}
