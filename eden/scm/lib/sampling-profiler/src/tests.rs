/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::thread;
use std::time::Duration;

use crate::Profiler;

#[test]
fn test_stress_concurrent_profilers() {
    // Repeat to surface intermittent hangs or crashes.

    // Note: each "Profiler" might spawn 1 to 2 threads.
    // Total: JOBS * INTERVALS.len() * (2 or 3) threads.
    let (jobs, iterations, intervals) = if cfg!(debug_assertions) {
        (4, 30, &[2][..])
    } else {
        (12, 50, &[2, 1, 0, 5][..])
    };

    let handles: Vec<_> = (0..jobs)
        .into_iter()
        .map(|i| {
            thread::spawn(move || {
                for j in 0..iterations {
                    eprintln!("Thread #{i}: Iteration {j}");
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
                    thread::sleep(Duration::from_millis(64));

                    // Try different drop orders.
                    for _ in 1..profilers.len() {
                        profilers.remove((i + j) % profilers.len());
                    }
                    drop(profilers);
                }
                eprintln!("Thread #{i}: Completed");
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}
