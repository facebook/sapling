/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[cfg(not(windows))]
use std::ffi::c_int;
use std::ffi::c_void;

unsafe extern "C" {
    #[link_name = "sigbus_is_protected"]
    fn ffi_is_protected() -> bool;

    #[link_name = "sigbus_try_memcpy"]
    fn ffi_try_memcpy(dst: *mut c_void, src: *const c_void, len: usize) -> bool;

    #[link_name = "sigbus_try_read"]
    fn ffi_try_read(src: *const c_void, len: usize) -> bool;

    #[cfg(not(windows))]
    #[link_name = "sigbus_try_handle"]
    fn ffi_try_handle(signo: c_int, info: *mut c_void, ucontext: *mut c_void) -> bool;
}

/// Returns whether SIGBUS protection is available on this target.
pub fn is_protected() -> bool {
    // SAFETY: This FFI function has no arguments or safety preconditions.
    unsafe { ffi_is_protected() }
}

/// Tries to copy `len` bytes from `src` to `dst`.
///
/// Returns `false` if a recognized synchronous SIGBUS interrupts the copy.
/// The destination may have been partially modified in that case.
///
/// # Safety
///
/// `src` and `dst` must each identify a mapped range of at least `len` bytes
/// and must not overlap. On protected platforms, the process SIGBUS handler
/// must call `try_handle` before delegating to its fallback handler.
pub unsafe fn try_memcpy(dst: *mut u8, src: *const u8, len: usize) -> bool {
    // SAFETY: By this function's contract, `dst` and `src` identify valid,
    // non-overlapping ranges of `len` bytes. Casting to `c_void` preserves
    // their addresses and provenance.
    unsafe { ffi_try_memcpy(dst.cast(), src.cast(), len) }
}

/// Tries to read `len` bytes starting at `src`.
///
/// Returns `false` if a recognized synchronous SIGBUS interrupts the read.
///
/// # Safety
///
/// `src` must identify a mapped range of at least `len` bytes. On protected
/// platforms, the process SIGBUS handler must call `try_handle` before
/// delegating to its fallback handler.
pub unsafe fn try_read(src: *const u8, len: usize) -> bool {
    // SAFETY: By this function's contract, `src` identifies a mapped range of
    // `len` bytes. Casting to `c_void` preserves its address and provenance.
    unsafe { ffi_try_read(src.cast(), len) }
}

/// Tries to redirect a synchronous SIGBUS raised by [`try_memcpy`] or
/// [`try_read`] to its recovery path.
///
/// # Safety
///
/// `info` and `ucontext` must be the pointers supplied to an SA_SIGINFO signal
/// handler for `signo`. When this returns `true`, the handler must return
/// immediately.
#[cfg(not(windows))]
pub unsafe fn try_handle(signo: c_int, info: *mut c_void, ucontext: *mut c_void) -> bool {
    // SAFETY: By this function's contract, `info` and `ucontext` are the exact
    // pointers supplied by the kernel for `signo`, as required by the C API.
    unsafe { ffi_try_handle(signo, info, ucontext) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_and_read_accessible_memory() {
        if !is_protected() {
            return;
        }

        let source = [1, 2, 3, 4, 5, 6, 7, 8];
        let mut destination = [0; 8];

        // SAFETY: Both arrays are valid, equally sized, and non-overlapping.
        assert!(unsafe { try_memcpy(destination.as_mut_ptr(), source.as_ptr(), source.len()) });
        assert_eq!(destination, source);
        // SAFETY: `source` is readable for its full length.
        assert!(unsafe { try_read(source.as_ptr(), source.len()) });
        // SAFETY: A zero-length read does not access the pointer.
        assert!(unsafe { try_read(std::ptr::null(), 0) });
    }

    #[cfg(unix)]
    #[test]
    fn catches_faults_past_end_of_file() {
        use std::mem::MaybeUninit;

        use memmap2::MmapOptions;

        if !is_protected() {
            return;
        }

        unsafe extern "C" fn on_sigbus(
            signo: c_int,
            info: *mut libc::siginfo_t,
            ucontext: *mut c_void,
        ) {
            // SAFETY: These pointers are supplied by the kernel to this
            // SA_SIGINFO signal handler.
            if unsafe { try_handle(signo, info.cast(), ucontext) } {
                return;
            }

            // SAFETY: `_exit` is async-signal-safe and does not return.
            unsafe { libc::_exit(128 + signo) }
        }

        struct RestoreSigbusHandler(libc::sigaction);

        impl Drop for RestoreSigbusHandler {
            fn drop(&mut self) {
                // SAFETY: `self.0` was initialized by `sigaction`, and a null
                // third argument is permitted when restoring an action.
                unsafe {
                    libc::sigaction(libc::SIGBUS, &self.0, std::ptr::null_mut());
                }
            }
        }

        // SAFETY: `_SC_PAGESIZE` is a valid `sysconf` selector.
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        assert!(page_size > 0);
        let page_size = page_size as usize;

        let file = tempfile::tempfile().unwrap();
        file.set_len(page_size as u64).unwrap();

        // SAFETY: The file remains open for the mapping lifetime. Mapping past
        // EOF is intentional so accesses to the second page raise SIGBUS.
        let mut mapping = unsafe { MmapOptions::new().len(2 * page_size).map_mut(&file) }.unwrap();

        let mut action = MaybeUninit::<libc::sigaction>::zeroed();
        let mut old_action = MaybeUninit::<libc::sigaction>::uninit();
        // SAFETY: Both sigaction values point to valid writable storage, and
        // the handler has the SA_SIGINFO function signature.
        unsafe {
            let action = action.assume_init_mut();
            action.sa_sigaction = on_sigbus as *const () as usize;
            action.sa_flags = libc::SA_SIGINFO;
            assert_eq!(libc::sigemptyset(&mut action.sa_mask), 0);
            assert_eq!(
                libc::sigaction(libc::SIGBUS, action, old_action.as_mut_ptr()),
                0
            );
        }
        // SAFETY: The successful `sigaction` call initialized `old_action`.
        let _restore_handler = RestoreSigbusHandler(unsafe { old_action.assume_init() });

        let source = [0x80; 16];
        // SAFETY: Both pointer ranges are mapped for 16 bytes and do not
        // overlap. Accessing the destination beyond EOF intentionally faults.
        assert!(!unsafe {
            try_memcpy(
                mapping.as_mut_ptr().add(page_size - 8),
                source.as_ptr(),
                source.len(),
            )
        });
        assert_eq!(&mapping[page_size - 8..page_size], &source[..8]);
        // SAFETY: The ranges are mapped, but accessing bytes beyond EOF
        // intentionally faults.
        assert!(!unsafe { try_read(mapping.as_ptr().add(page_size), 1) });
        assert!(!unsafe { try_read(mapping.as_ptr().add(page_size - 8), 16) });
    }
}
