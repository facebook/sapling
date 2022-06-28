/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::thread;

use chrono::Local;
use context::PerfCounterType;
use context::PerfCounters;

fn main() {
    let ctrs = Arc::new(PerfCounters::default());
    let k = PerfCounterType::BlobGets;
    let k2 = PerfCounterType::BlobPuts;

    let start = Local::now();
    println!("Start: {}", start);

    let n_threads = 10;
    let n_ops = 100000;

    let threads = (0..n_threads).map(|_| {
        thread::spawn({
            let ctrs = ctrs.clone();
            move || {
                for i in 0..n_ops {
                    ctrs.increment_counter(k);
                    ctrs.set_max_counter(k2, i);
                }
            }
        })
    });

    for t in threads {
        t.join().unwrap();
    }

    let done = Local::now();

    assert_eq!(ctrs.get_counter(k), n_threads * n_ops);
    assert_eq!(ctrs.get_counter(k2), n_ops - 1);

    println!("Elapsed: {}ms", (done - start).num_milliseconds());
}
