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

    let backtrace_id: usize = {
        static BACKTRACE_ID: AtomicUsize = AtomicUsize::new(0);
        BACKTRACE_ID.fetch_add(1, Ordering::AcqRel)
    };
    let mut depth = 0;

    // Skip the first 2 frames:
    // - This signal handler frame.
    // - __sigaction
    const SKIP_FRAMES: usize = 2;
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
}

/// Write a `MaybeFrame`. Handles EINTR.
/// Return 0 on success. Return errno otherwise.
///
/// This function is to be called from a signal handler, and intentionally
/// avoids high-level Rust types like `io::Result`. So its easier to audit
/// async-signal-safety.
fn write_frame(frame: &FramePayload, fd: libc::c_int) -> libc::c_int {
    let size = std::mem::size_of::<FramePayload>();
    let pos = frame as *const FramePayload as *const libc::c_void;
    loop {
        // safety: FramePayload is `repr(C)` and contains only `usize` fields.
        // It is okay to write its raw bytes to "serialize" within the same process.
        let written_bytes = unsafe { libc::write(fd, pos, size) };
        if written_bytes < 0 {
            let errno = {
                #[cfg(target_os = "macos")]
                unsafe {
                    *libc::__error()
                }

                #[cfg(target_os = "linux")]
                unsafe {
                    *libc::__errno_location()
                }

                #[cfg(not(any(target_os = "linux", target_os = "macos")))]
                libc::EINVAL
            };
            if errno == libc::EINTR {
                // Retry
                continue;
            } else {
                return errno;
            }
        } else if written_bytes as usize == size {
            return 0;
        } else {
            return libc::EINVAL;
        }
    }
}
