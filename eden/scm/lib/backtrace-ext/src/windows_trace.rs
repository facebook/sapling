/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Capture backtraces from another thread on Windows via thread suspension.
//!
//! This is the Windows counterpart to signal handler + `trace_unsynchronized!`
//! combination on Unix for profiling use-cases. Windows does not have signal
//! handlers, we suspend the target externally, walk its stack, then resume it.

use std::io;

use winapi::shared::minwindef::DWORD;
use winapi::shared::minwindef::FALSE;
use winapi::shared::ntdef::HANDLE;
use winapi::um::handleapi::CloseHandle;
use winapi::um::handleapi::DuplicateHandle;
use winapi::um::processthreadsapi::GetCurrentProcess;
use winapi::um::processthreadsapi::GetCurrentThread;

/// RAII wrapper for a duplicated Windows thread HANDLE.
///
/// Created via `DuplicateHandle` of the current thread's pseudo-handle,
/// producing a real handle that can be used from any thread.
pub struct ThreadHandle(HANDLE);

// HANDLEs are process-wide identifiers, safe to send across threads.
unsafe impl Send for ThreadHandle {}

impl ThreadHandle {
    /// Capture a duplicated handle to the calling thread.
    pub fn current_thread() -> io::Result<Self> {
        unsafe {
            let mut handle: HANDLE = std::ptr::null_mut();
            let process = GetCurrentProcess();
            // THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT
            let desired_access: DWORD = 0x0002 | 0x0008;
            let ok = DuplicateHandle(
                process,
                GetCurrentThread(),
                process,
                &mut handle,
                desired_access,
                FALSE,
                0,
            );
            if ok == 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(Self(handle))
            }
        }
    }
}

impl Drop for ThreadHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[cfg(target_arch = "x86_64")]
mod imp {
    use winapi::shared::minwindef::DWORD;
    use winapi::shared::ntdef::HANDLE;
    use winapi::um::processthreadsapi::GetThreadContext;
    use winapi::um::processthreadsapi::ResumeThread;
    use winapi::um::processthreadsapi::SuspendThread;
    use winapi::um::winnt::CONTEXT;
    use winapi::um::winnt::CONTEXT_FULL;
    use winapi::um::winnt::RtlLookupFunctionEntry;
    use winapi::um::winnt::RtlVirtualUnwind;

    use crate::Frame;
    use crate::FrameDecision;
    use crate::ThreadHandle;
    use crate::get_supplemental_frame_resolver;

    const MAX_FRAMES: usize = 4096;

    /// Capture a backtrace from another thread.
    ///
    /// Suspends the thread, walks its stack using `RtlVirtualUnwind`,
    /// extracts supplemental info (e.g. Python frames), then resumes it.
    /// Returns frames in most-recent-call-first order.
    ///
    /// Symbol resolution happens *after* the thread is resumed — only the
    /// fast, fixed-size `Frame` data is collected while suspended.
    pub fn trace_remote_thread(handle: &ThreadHandle) -> Vec<Frame> {
        // Allocate before suspending. The suspended thread may hold the
        // allocator lock, so any alloc/free while it is frozen deadlocks.
        let mut ctx: Box<CONTEXT> = Box::new(unsafe { std::mem::zeroed() });
        let mut frames = Vec::with_capacity(MAX_FRAMES);

        // === Suspension window (no alloc/free allowed) ==================
        // Safety: ThreadHandle holds a valid, duplicated thread handle.
        unsafe {
            if SuspendThread(handle.0) != DWORD::MAX {
                walk_stack(handle.0, &mut ctx, &mut frames);
                ResumeThread(handle.0);
            }
        }
        // === End suspension window ======================================

        // ctx and frames are freed here, after the target is resumed.
        frames
    }

    /// Walk the suspended thread's stack via RtlVirtualUnwind.
    ///
    /// All buffers are pre-allocated by the caller so this function
    /// performs zero heap allocations (or frees) while the target is frozen.
    ///
    /// # Safety
    /// `thread` must be a valid, suspended thread handle.
    unsafe fn walk_stack(thread: HANDLE, ctx: &mut Box<CONTEXT>, frames: &mut Vec<Frame>) {
        ctx.ContextFlags = CONTEXT_FULL;

        if unsafe { GetThreadContext(thread, ctx.as_mut() as *mut CONTEXT) } == 0 {
            return;
        }

        let resolver = get_supplemental_frame_resolver();

        for _ in 0..frames.capacity() {
            let ip = ctx.Rip as usize;
            if ip == 0 {
                break;
            }
            let sp = ctx.Rsp as usize;

            let decision = match &resolver {
                Some(r) => r.maybe_extract_supplemental_info(ip, sp),
                None => FrameDecision::Keep,
            };

            match decision {
                FrameDecision::Keep => {
                    frames.push(Frame { ip, sp, info: None });
                }
                FrameDecision::Skip => {}
                FrameDecision::Replace(info) => {
                    frames.push(Frame {
                        ip,
                        sp,
                        info: Some(info),
                    });
                }
            }

            // Unwind one frame.
            let mut image_base: u64 = 0;
            let fn_entry =
                unsafe { RtlLookupFunctionEntry(ctx.Rip, &mut image_base, std::ptr::null_mut()) };
            if fn_entry.is_null() {
                // Leaf function: pop return address from stack.
                if ctx.Rsp == 0 {
                    break;
                }
                ctx.Rip = unsafe { *(ctx.Rsp as *const u64) };
                ctx.Rsp = ctx.Rsp.wrapping_add(8);
                continue;
            }

            let mut handler_data: *mut std::ffi::c_void = std::ptr::null_mut();
            let mut establisher_frame: u64 = 0;
            unsafe {
                RtlVirtualUnwind(
                    0, // UNW_FLAG_NHANDLER
                    image_base,
                    ctx.Rip,
                    fn_entry,
                    ctx.as_mut() as *mut CONTEXT,
                    &mut handler_data,
                    &mut establisher_frame,
                    std::ptr::null_mut(),
                )
            };
        }
    }
}

#[cfg(not(target_arch = "x86_64"))]
mod imp {
    use crate::Frame;
    use crate::ThreadHandle;

    /// Capture a backtrace from another thread.
    ///
    /// Unsupported on this architecture: returns no frames.
    pub fn trace_remote_thread(_handle: &ThreadHandle) -> Vec<Frame> {
        Vec::new()
    }
}

pub use imp::trace_remote_thread;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_remote_thread_captures_frames() {
        /// Records its own (fn_addr, sp_hint) marker, then calls `next`.
        /// Each `N` produces a distinct monomorphization with a unique address.
        #[inline(never)]
        fn target_function<const N: usize>(
            markers: &mut Vec<(usize, usize)>,
            next: &dyn Fn(&mut Vec<(usize, usize)>),
        ) {
            let anchor = 0usize;
            markers.push((target_function::<N> as usize, &anchor as *const _ as usize));
            (*next)(markers);
        }

        let (tx, rx) = std::sync::mpsc::channel();

        let remote_thread = std::thread::spawn(move || {
            target_function::<1>(&mut Vec::new(), &|m| {
                target_function::<2>(m, &|m| {
                    target_function::<3>(m, &|m| {
                        let handle = ThreadHandle::current_thread().unwrap();
                        tx.send((handle, std::mem::take(m))).unwrap();
                        // This thread stays alive while the parent thread takes its backtrace.
                        std::thread::park();
                    })
                })
            });
        });

        let (handle, markers) = rx.recv().unwrap();
        let frames = trace_remote_thread(&handle);
        assert!(frames.len() >= 3, "expected at least 3 frames");

        const IP_THRESHOLD: usize = 1110; // example real-world ip_offset: 111
        const SP_THRESHOLD: usize = 560; // example real-world sp_distance: 56
        for (fn_addr, sp_hint) in &markers {
            // IP is a return address within the function body, so >= entry.
            let target_frame = frames
                .iter()
                .filter(|f| f.ip >= *fn_addr)
                .min_by_key(|f| f.ip - fn_addr)
                .unwrap_or_else(|| panic!("no frame with IP >= {fn_addr:#x}"));

            let ip_offset = target_frame.ip - fn_addr;
            assert!(
                ip_offset < IP_THRESHOLD,
                "IP {:#x} too far from function {:#x} (offset {ip_offset})",
                target_frame.ip,
                fn_addr,
            );

            let sp_distance = (target_frame.sp as isize - *sp_hint as isize).unsigned_abs();
            assert!(
                sp_distance < SP_THRESHOLD,
                "SP {:#x} too far from stack anchor {:#x} (distance {sp_distance})",
                target_frame.sp,
                sp_hint,
            );
        }

        remote_thread.thread().unpark();
        remote_thread.join().unwrap();
    }
}
