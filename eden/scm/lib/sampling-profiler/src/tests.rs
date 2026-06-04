/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::thread;
use std::time::Duration;
use std::time::Instant;

use crate::Profiler;

// Run with output:
// reset; cargo test --release --lib -- --nocapture  stress
#[test]
fn test_stress_concurrent_profilers() {
    // Repeat to surface intermittent hangs or crashes.

    // Note: each "Profiler" spawns 1 thread.
    // Total: JOBS * INTERVALS.len() * 2 threads.
    let (jobs, iterations, intervals) = if cfg!(debug_assertions) {
        (4, 30, &[2][..])
    } else {
        // Avoid zero interval - it can cause `sleep` to EINTR and never finish
        // due to SA_RESTART.
        (6, 30, &[2, 1, 5][..])
    };

    eprint!("{}", "\n".repeat(jobs));
    fn eprint_at(line_no: usize, message: String) {
        // Go up, clear, write, go down.
        let s = format!("\x1b[{line_no}A\r\x1b[K{message}\x1b[{line_no}B\r");
        eprint!("{s}");
    }

    let handles: Vec<_> = (0..jobs)
        .map(|i| {
            thread::spawn(move || {
                let line_no = jobs - i;
                let log = |s| eprint_at(line_no, format!("Thread {i}: {s}"));
                for j in 0..iterations {
                    let log = |s| log(format!("Iteration {j}: {s}"));
                    log("Creating profilers".into());
                    let mut profilers: Vec<_> = intervals
                        .iter()
                        .map(|&ms| {
                            Profiler::new(
                                Duration::from_millis(ms),
                                Box::new(move |_frames: &[String]| {}),
                            )
                            .unwrap()
                        })
                        .collect();

                    // Rust stdlib has assertion on "errno" (some libc function seems to use errno
                    // instead of a local state). This sleep might exercise the errno save/restore
                    // logic in the signal handler.
                    // https://github.com/rust-lang/rust/blob/5dbaac135785bca7152c5809430b1fb1653db8b1/library/std/src/sys/thread/unix.rs#L590
                    log("Sleeping".into());
                    thread::sleep(Duration::from_millis(64));

                    // Try different drop orders.
                    for k in 1..profilers.len() {
                        log(format!("Dropping Profiler #{k}"));
                        profilers.remove((i + j) % profilers.len());
                    }
                    log("Dropping all Profilers".into());
                    drop(profilers);
                }
                log("Completed".into());
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

/// Stress test about malloc use-cases (which uses locks internally) where
/// a signal handler that calls malloc-dependent functions can deadlock.
///
/// With the ring buffer approach, the signal handler only uses async-signal-safe
/// operations (atomic loads/stores and ptr::write), so this specific deadlock
/// is eliminated. This test remains as a general stress test.
#[test]
fn test_signal_during_malloc_deadlock() {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;

    let stop = Arc::new(AtomicBool::new(false));

    let worker = thread::spawn({
        let stop = stop.clone();
        move || {
            let _profiler = Profiler::new(
                Duration::from_millis(1),
                Box::new(|_: &[String]| {
                    // Allocation-heavy callback slows the consumer, filling the
                    // ring buffer and increasing pressure — useful for stress
                    // testing the lock-free ring buffer under contention.
                    let mut v = Vec::new();
                    for i in 0..200 {
                        v.push(vec![0u8; 512 * (i + 1)]);
                    }
                }),
            )
            .unwrap();

            // Churn allocations so SIGPROF likely fires inside jemalloc.
            while !stop.load(Ordering::Relaxed) {
                let mut bufs: Vec<Vec<u8>> = Vec::with_capacity(256);
                for size in (1..=128).map(|i| i * 512) {
                    bufs.push(vec![0u8; size]);
                }
                while bufs.len() > 1 {
                    bufs.swap_remove(bufs.len() / 2);
                }
            }
            // _profiler drops here. If deadlocked, we never reach this point.
        }
    });

    // Let the workload run under profiling pressure.
    thread::sleep(Duration::from_secs(3));
    stop.store(true, Ordering::Relaxed);

    // If the worker (or its Profiler::drop) is deadlocked, it won't finish.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !worker.is_finished() {
        assert!(
            Instant::now() < deadline,
            "deadlock: signal handler write blocked while consumer waits on allocator lock"
        );
        thread::sleep(Duration::from_millis(200));
    }
    worker.join().unwrap();
}

#[test]
fn test_profiler_drop_is_fast() {
    let interval = Duration::from_secs(5);
    let start = Instant::now();
    let profiler = Profiler::new(interval, Box::new(|_: &[String]| {})).unwrap();
    std::thread::sleep(Duration::from_millis(10));
    drop(profiler);
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(2),
        "drop took {elapsed:?}, expected < 2s",
    );
}
