/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::mem;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::OnceLock;

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
    new_action.sa_sigaction = signal_handler as usize;
    new_action.sa_flags = libc::SA_SIGINFO;
    tracing::debug!("registering SIGBUS handler");
    unsafe {
        ORIG_HANDLER = Some(mem::zeroed());
        if let Some(old_handler_mut) = ORIG_HANDLER.as_mut() {
            libc::sigaction(libc::SIGBUS, &new_action, old_handler_mut);
        }
    }
}

unsafe extern "C" fn signal_handler(
    sig: libc::c_int,
    info: *mut libc::siginfo_t,
    _ucontext: usize,
) {
    if let Some(info) = info.as_ref() {
        let addr = info.si_addr() as usize;
        // ASYNC SIGNAL SAFETY: This is not "async signal safe" in theory. However, to make
        // it async signal safe it typically means extra pipes, threads, more complexity with
        // `fork`, etc. We're crashing (and in relatively rare cases) anyway, so don't bother
        // async signal safety for now.
        if crate::page_out::find_region(addr).is_some() && zero_fill_page(addr).is_ok() {
            // Retry, since zero_fill_page probably made it readable.
            return;
        }
    }

    // Fallback to the original handler. Restore the old handler and re-raise the signal.
    // This can happen when (but not limited to):
    // - The address in question is not tracked by indexedlog's (file-backed) mmap buffers.
    // - Already tried fixing the same page before, to prevent infinite loop.
    if let Some(old_handler_mut) = ORIG_HANDLER.as_mut() {
        libc::sigaction(sig, old_handler_mut, std::ptr::null_mut());
        // Retry as a way to re-raise.
    }
}

/// Zero-fill a page that contains the given address, to make it readable.
fn zero_fill_page(addr: usize) -> Result<(), ()> {
    static PAGE_SIZE: OnceLock<i64> = OnceLock::new();
    let page_size = *PAGE_SIZE.get_or_init(|| unsafe { libc::sysconf(libc::_SC_PAGESIZE) });
    if page_size <= 0 {
        return Err(());
    }

    let page_size = page_size as usize;
    let start: usize = addr / page_size * page_size;

    static LAST_START: AtomicUsize = AtomicUsize::new(0);
    let last_start = LAST_START.swap(start, Ordering::AcqRel);
    if last_start == start {
        // Just attempted fixing this page. Do not try again.
        return Err(());
    }

    // Use mmap MAP_FIXED | MAP_ANONYMOUS to zero-fill the page.
    let prot = libc::PROT_READ;
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
    use std::fs::OpenOptions;

    use tempfile::tempdir;

    use crate::log::Log;
    use crate::log::PRIMARY_FILE;

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
}
