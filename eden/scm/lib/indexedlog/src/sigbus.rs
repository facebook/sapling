/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::c_void;
use std::mem;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

static mut ORIG_HANDLER: Option<libc::sigaction> = None;

/// Register a SIGBUS signal handler that attempts to avoid program crash caused
/// by indexedlog-maintained mmap buffers.
///
/// SIGBUS can practically happen when btrfs failed to verify the btrfs-level
/// checksum. It would be easier if we can tell btrfs/mmap to simply return
/// zeros to us, so our xxhash checksum will detect problems, instead of raising
/// SIGBUS. However, Linux/btrfs mmap does not yet have this feature at the time
/// of writing.
///
/// So we emulate this behavior by zero-filling bad pages ourselves.
pub fn register_sigbus_handler() {
    static REGISTERED: AtomicBool = AtomicBool::new(false);
    if REGISTERED.fetch_or(true, Ordering::AcqRel) {
        return;
    }
    crate::page_out::NEED_FIND_REGION.store(true, Ordering::Release);
    let mut new_action: libc::sigaction = unsafe { mem::zeroed() };
    new_action.sa_sigaction = signal_handler as *const () as usize;
    new_action.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK;
    tracing::debug!("registering SIGBUS handler");
    unsafe {
        ORIG_HANDLER = Some(mem::zeroed());
        #[expect(static_mut_refs)]
        if let Some(old_handler_mut) = ORIG_HANDLER.as_mut() {
            libc::sigaction(libc::SIGBUS, &new_action, old_handler_mut);
        }
    }
}

unsafe extern "C" fn signal_handler(
    sig: libc::c_int,
    info: *mut libc::siginfo_t,
    ucontext: *mut c_void,
) {
    unsafe {
        if let Some(info) = info.as_ref() {
            let addr = info.si_addr() as usize;
            // ASYNC SIGNAL SAFETY: This is not "async signal safe" in theory. However, to make
            // it async signal safe it typically means extra pipes, threads, more complexity with
            // `fork`, etc. We're crashing (and in relatively rare cases) anyway, so don't bother
            // async signal safety for now.
            if let Some((_start, _end, writable)) = crate::page_out::find_region(addr) {
                if zero_fill_page(addr, writable).is_ok() {
                    // Retry, since zero_fill_page probably made it accessible.
                    return;
                }
            }
        }

        // Call a previous handler directly so multiple recoverable SIGBUS
        // handlers can coexist. An ignored or default disposition still has to
        // be restored so the fault can be raised again by the kernel.
        // This can happen when (but not limited to):
        // - The address in question is not tracked by indexedlog's (file-backed) mmap buffers.
        // - Already tried fixing the same page before, to prevent infinite loop.
        #[expect(static_mut_refs)]
        if let Some(old_handler) = ORIG_HANDLER.as_ref() {
            call_original_handler(old_handler, sig, info, ucontext);
        }
    }
}

unsafe fn call_original_handler(
    action: &libc::sigaction,
    sig: libc::c_int,
    info: *mut libc::siginfo_t,
    ucontext: *mut c_void,
) {
    let handler = action.sa_sigaction;
    if handler == libc::SIG_IGN || handler == libc::SIG_DFL {
        unsafe {
            libc::sigaction(sig, action, std::ptr::null_mut());
        }
        return;
    }

    if action.sa_flags & libc::SA_SIGINFO != 0 {
        let handler: unsafe extern "C" fn(libc::c_int, *mut libc::siginfo_t, *mut c_void) =
            unsafe { mem::transmute(handler) };
        unsafe { handler(sig, info, ucontext) };
    } else {
        let handler: unsafe extern "C" fn(libc::c_int) = unsafe { mem::transmute(handler) };
        unsafe { handler(sig) };
    }
}

/// Zero-fill a page that contains the given address, to make it readable.
fn zero_fill_page(addr: usize, writable: bool) -> Result<(), ()> {
    let page_size = crate::page_out::page_size().ok_or(())?;
    let start: usize = addr / page_size * page_size;

    static LAST_START: AtomicUsize = AtomicUsize::new(0);
    let last_start = LAST_START.swap(start, Ordering::AcqRel);
    if last_start == start {
        // Just attempted fixing this page. Do not try again.
        return Err(());
    }

    // Use mmap MAP_FIXED | MAP_ANONYMOUS to zero-fill the page.
    let mut prot = libc::PROT_READ;
    if writable {
        prot |= libc::PROT_WRITE
    }
    let flags = libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_FIXED;
    let fd = -1; // With MAP_ANONYMOUS, fd is not used.
    let offset = 0;
    let mmap_ret = unsafe { libc::mmap(start as _, page_size, prot, flags, fd, offset) };
    if mmap_ret == libc::MAP_FAILED {
        return Err(());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::ffi::c_void;
    use std::fs::OpenOptions;
    use std::mem;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use tempfile::tempdir;

    use crate::log::Log;
    use crate::log::PRIMARY_FILE;

    #[test]
    fn test_call_original_siginfo_handler() {
        static CALLS: AtomicUsize = AtomicUsize::new(0);

        unsafe extern "C" fn handler(
            sig: libc::c_int,
            _info: *mut libc::siginfo_t,
            _ucontext: *mut c_void,
        ) {
            assert_eq!(sig, libc::SIGBUS);
            CALLS.fetch_add(1, Ordering::Relaxed);
        }

        let mut action: libc::sigaction = unsafe { mem::zeroed() };
        action.sa_sigaction = handler as *const () as usize;
        action.sa_flags = libc::SA_SIGINFO;
        unsafe {
            super::call_original_handler(
                &action,
                libc::SIGBUS,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            super::call_original_handler(
                &action,
                libc::SIGBUS,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        }
        assert_eq!(CALLS.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_call_original_ignored_handler_restores_disposition() {
        unsafe extern "C" fn handler(_sig: libc::c_int) {}

        let mut current_action: libc::sigaction = unsafe { mem::zeroed() };
        current_action.sa_sigaction = handler as *const () as usize;
        let mut ignored_action: libc::sigaction = unsafe { mem::zeroed() };
        ignored_action.sa_sigaction = libc::SIG_IGN;
        let mut original_action: libc::sigaction = unsafe { mem::zeroed() };

        let signal = libc::SIGUSR2;
        unsafe {
            assert_eq!(
                libc::sigaction(signal, &current_action, &mut original_action),
                0
            );
            super::call_original_handler(
                &ignored_action,
                signal,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        }

        let mut installed_action: libc::sigaction = unsafe { mem::zeroed() };
        let query_result =
            unsafe { libc::sigaction(signal, std::ptr::null(), &mut installed_action) };
        unsafe {
            libc::sigaction(signal, &original_action, std::ptr::null_mut());
        }
        assert_eq!(query_result, 0);
        assert_eq!(installed_action.sa_sigaction, libc::SIG_IGN);
    }

    #[test]
    fn test_sigbus_truncate_log() {
        // Commenting this out and this test will crash with SIGBUS.
        super::register_sigbus_handler();

        let dir = tempdir().unwrap();
        let log_path = dir.path();

        // Write some data.
        const L: usize = 500;
        let mut log = Log::open(log_path, Vec::new()).unwrap();
        for i in 0..50u8 {
            let data = [i; L];
            log.append(data).unwrap();
        }
        log.sync().unwrap();

        // All data should be readable.
        for (i, entry) in log.iter().enumerate() {
            let data = entry.unwrap();
            assert_eq!(data.len(), L);
            assert!(data.iter().all(|&d| d == i as u8));
        }

        // Break the Log by truncating the primary file in the middle.
        // Recreate the log right before truncation to "page out" the buffers.
        //
        // Note: Practically the problem is about btrfs checksum failures, which
        // is hard to emulate in this test. So we use truncation as an approx.
        let primary_path = log_path.join(PRIMARY_FILE);
        let mut opts = OpenOptions::new();
        let opts = opts.write(true).read(true).truncate(false);
        let file = opts.open(primary_path).unwrap();
        let orig_len = file.metadata().unwrap().len();
        let log = Log::open(log_path, Vec::new()).unwrap();
        for truncate_size_base in [orig_len - 1, orig_len / 2, 4096] {
            for truncate_size_delta in 0..4096 {
                let truncate_size = truncate_size_base - truncate_size_delta;
                file.set_len(truncate_size).unwrap();
                // Now only part of the data can be read. Reading the "truncated" entries will error out,
                // but not crash with SIGBUS.
                let mut error_count = 0;
                for (i, entry) in log.iter().enumerate() {
                    match entry {
                        Ok(data) => {
                            // For "Ok" entries, they should have the right content.
                            assert_eq!(data.len(), L);
                            assert!(data.iter().all(|&d| d == i as u8));
                        }
                        Err(_e) => {
                            error_count += 1;
                        }
                    }
                }
                assert!(error_count > 0);
            }
        }
    }

    #[test]
    fn test_sigbus_truncate_rlock_change_detector() {
        super::register_sigbus_handler();

        let dir = tempdir().unwrap();
        let log_path = dir.path().join("log");
        let mut log1 = Log::open(&log_path, Vec::new()).unwrap();
        let log2 = Log::open(&log_path, Vec::new()).unwrap();

        let file = OpenOptions::new()
            .write(true)
            .read(true)
            .open(log_path.join("rlock"))
            .unwrap();
        file.set_len(0).unwrap();

        log1.append([b'a'; 10]).unwrap();
        log1.sync().unwrap();

        assert!(!log1.is_changed_on_disk());
        assert!(log2.is_changed_on_disk());
    }
}
