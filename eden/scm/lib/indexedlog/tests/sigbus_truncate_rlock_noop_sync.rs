/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[cfg(target_os = "linux")]
mod linux_tests {
    use std::fs;
    use std::fs::OpenOptions;
    use std::path::Path;

    use indexedlog::log::Log;
    use tempfile::tempdir;

    #[test]
    fn test_sigbus_truncate_rlock_noop_sync_without_handler() {
        let mut status = 0;
        // SAFETY: `fork` isolates the expected SIGBUS in the child process.
        // The child exits with `_exit` instead of returning through Rust cleanup.
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0);
        if pid == 0 {
            // SAFETY: Resetting the handler only affects the child process.
            unsafe { libc::signal(libc::SIGBUS, libc::SIG_DFL) };
            truncated_rlock_noop_sync();
            // SAFETY: The child must terminate without running Rust cleanup after `fork`.
            unsafe { libc::_exit(0) };
        }

        // SAFETY: `pid` is the positive child pid returned by `fork`, and
        // `status` points to valid writable memory for `waitpid`.
        let wait_result = unsafe { libc::waitpid(pid, &mut status, 0) };
        assert_eq!(wait_result, pid);
        // FIXME: This should be `assert!(libc::WIFEXITED(status))` once no-op
        // sync avoids touching an unchanged rlock change detector.
        assert!(
            libc::WIFSIGNALED(status),
            "expected child to terminate from SIGBUS, got status {status}",
        );
        assert_eq!(libc::WTERMSIG(status), libc::SIGBUS);
    }

    fn truncated_rlock_noop_sync() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("log");
        let mut log = Log::open(&log_path, Vec::new()).unwrap();

        log.append([b'a'; 10]).unwrap();
        log.sync().unwrap();

        let rlock_path = log_path.join("rlock");
        let file = OpenOptions::new()
            .write(true)
            .read(true)
            .open(&rlock_path)
            .unwrap();
        file.set_len(0).unwrap();
        discard_mapping(&rlock_path);

        log.sync().unwrap();
    }

    fn discard_mapping(path: &Path) {
        let target = path.to_string_lossy();
        let maps = fs::read_to_string("/proc/self/maps").unwrap();
        for line in maps.lines() {
            let mut parts = line.split_whitespace();
            let Some(range) = parts.next() else {
                continue;
            };
            if parts.nth(4) != Some(target.as_ref()) {
                continue;
            }

            let (start, end) = range.split_once('-').unwrap();
            let start = usize::from_str_radix(start, 16).unwrap();
            let end = usize::from_str_radix(end, 16).unwrap();
            // SAFETY: The address range comes from `/proc/self/maps` for the
            // live `rlock` mmap in this process. `MADV_DONTNEED` discards the
            // resident page so the next access faults against the truncated file.
            let ret = unsafe {
                libc::madvise(start as *mut libc::c_void, end - start, libc::MADV_DONTNEED)
            };
            assert_eq!(ret, 0);
            return;
        }

        panic!("could not find mmap for {target}");
    }
}
