// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// Test RotateLog behavior when fd is limited.

#[cfg(unix)]
mod unix_tests {
    use indexedlog::log::{IndexDef, IndexOutput};
    use indexedlog::rotate::OpenOptions;
    use tempfile::tempdir;

    const MAX_NOFILE: libc::rlim_t = 256;

    #[test]
    fn test_low_fileno_limit() {
        let verbose = std::env::var("VERBOSE").is_ok();
        for i in 10..30 {
            if verbose {
                eprintln!("Testing RLIMIT_NOFILE = {}", i);
            }
            set_rlimit_nofile(i);
            test_multithread_sync()
        }
    }

    // Test writing using multi-threads. Verify that although some threads error
    // out, the resulting on-disk state is still consistent - data can be opened and
    // read if fileno limit is lifted.
    fn test_multithread_sync() {
        let verbose = std::env::var("VERBOSE").is_ok();
        let dir = tempdir().unwrap();

        // Release mode runs much faster.
        #[cfg(debug_assertions)]
        const THREAD_COUNT: u8 = 10;
        #[cfg(not(debug_assertions))]
        const THREAD_COUNT: u8 = 30;

        #[cfg(debug_assertions)]
        const WRITE_COUNT_PER_THREAD: u8 = 10;
        #[cfg(not(debug_assertions))]
        const WRITE_COUNT_PER_THREAD: u8 = 50;

        // Some indexes. They have different lag_threshold.
        fn index_ref(data: &[u8]) -> Vec<IndexOutput> {
            vec![IndexOutput::Reference(0..data.len() as u64)]
        }
        let indexes = vec![IndexDef::new("key1", index_ref).lag_threshold(1)];
        let index_len = indexes.len();
        let open_opts = OpenOptions::new()
            .create(true)
            .max_log_count(20)
            .max_bytes_per_log(50)
            .index_defs(indexes);

        use std::sync::{Arc, Barrier};
        let barrier = Arc::new(Barrier::new(THREAD_COUNT as usize));
        let threads: Vec<_> = (0..THREAD_COUNT)
            .map(|i| {
                let barrier = barrier.clone();
                let open_opts = open_opts.clone();
                let path = dir.path().join("rotatelog");
                std::thread::spawn(move || {
                    barrier.wait();
                    let run = || -> indexedlog::Result<()> {
                        // This might fail with fileno limit
                        let mut log = open_opts.clone().open(&path)?;
                        for j in 1..=WRITE_COUNT_PER_THREAD {
                            let buf = [i, j];
                            log.append(&buf).expect("append should not fail");
                            if j % (i + 1) == 0 || j == WRITE_COUNT_PER_THREAD {
                                // This might fail with fileno limit
                                log.sync()?;
                            }
                            if j % (i + 2) == 0 {
                                // Reopen log. This might fail with fileno limit.
                                log = open_opts.clone().open(&path)?;
                            }
                        }
                        Ok(())
                    };
                    match run() {
                        Ok(_) => (),
                        Err(err) => {
                            if verbose {
                                eprintln!(
                                    " thread {}: {}",
                                    i,
                                    format!("{:?}", err)
                                        .replace("\n\n", "\n")
                                        .replace("\n", "\n  ")
                                )
                            }
                        }
                    }
                })
            })
            .collect();

        // Wait for them. Some of the threads might fail.
        for thread in threads {
            thread.join().expect("joined");
        }

        // Check that if rlimit is restored, then the log can still be opened, and the indexes are
        // functional.
        set_rlimit_nofile(256);
        let log = open_opts.open(dir.path()).unwrap();
        for entry in log.iter().map(|d| d.unwrap()) {
            for index_id in 0..index_len {
                for index_value in log.lookup(index_id, entry).unwrap() {
                    assert_eq!(index_value.unwrap(), entry);
                }
            }
        }
    }

    fn set_rlimit_nofile(n: libc::rlim_t) {
        unsafe {
            let limit = libc::rlimit {
                rlim_cur: n,
                rlim_max: MAX_NOFILE,
            };
            libc::setrlimit(libc::RLIMIT_NOFILE, &limit as *const libc::rlimit);
        }
    }
}
