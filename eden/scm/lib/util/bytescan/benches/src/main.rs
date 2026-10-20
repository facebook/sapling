/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Benchmark `bytescan` against the std expressions it replaced.
//!
//! "old" is the replaced expression: `<[u8]>::contains(&0)` for `is_binary`
//! and `split(|c| c == &b'\n').count()` for `count_newlines`. "new" is
//! `bytescan`.
//!
//! Every measurement scans 256 MiB in total (a buffer of the given size,
//! scanned repeatedly), so times are directly comparable across sizes.
//! Pass substrings (e.g. `is_binary`, `mid-NUL` or `64MiB`) to run a subset
//! of rows.

use std::hint::black_box;

use minibench::Measure;
use minibench::bench_enabled;
use minibench::elapsed;

const TOTAL: usize = 256 << 20;

const SIZES: [(&str, usize); 4] = [
    ("4KiB", 4 << 10),
    ("64KiB", 64 << 10),
    ("1MiB", 1 << 20),
    ("64MiB", 64 << 20),
];

/// Buffers of each size sharing one kind of content.
struct Input {
    name: &'static str,
    buffers: Vec<(&'static str, Vec<u8>)>,
}

// Scans are taken as generic `impl Fn` rather than `fn` pointers so each one
// is inlined like at its real call site; a `fn` pointer makes the
// `split().count()` baseline ~1.7x slower and inflates the speedup.

fn scan_all(data: &[u8], scan: impl Fn(&[u8]) -> usize) {
    for _ in 0..TOTAL / data.len() {
        black_box(scan(black_box(data)));
    }
}

/// Best wall-clock time in milliseconds, using minibench's repeat policy.
fn best_ms(data: &[u8], scan: impl Fn(&[u8]) -> usize) -> f64 {
    let run = || elapsed(|| scan_all(data, &scan)).expect("WallClock::measure is infallible");
    let mut measured = run();
    while measured.need_more() {
        measured = measured.merge(run());
    }
    measured.best * 1e3
}

/// Prints one group of rows comparing the `old` and `new` scans on each buffer.
fn table(
    function: &str,
    input: &Input,
    old: impl Fn(&[u8]) -> usize,
    new: impl Fn(&[u8]) -> usize,
) {
    let enabled: Vec<_> = input
        .buffers
        .iter()
        .filter(|(size, _)| bench_enabled(&format!("{function} {} {size}", input.name)))
        .collect();
    if enabled.is_empty() {
        return;
    }

    println!();
    for (size, data) in enabled {
        let old_ms = best_ms(data, &old);
        let new_ms = best_ms(data, &new);
        println!(
            "{function:<14}  {:<7}  {size:>6} {old_ms:>7.2} ms {new_ms:>7.2} ms {:>7.1}x",
            input.name,
            old_ms / new_ms,
        );
    }
}

fn main() {
    // No NUL, so every scan walks the whole buffer.
    let no_nul = Input {
        name: "no-NUL",
        buffers: SIZES
            .iter()
            .map(|&(size, len)| {
                let text = b"the quick brown fox jumps over the lazy dog\n"
                    .iter()
                    .copied()
                    .cycle()
                    .take(len)
                    .collect();
                (size, text)
            })
            .collect(),
    };
    // A NUL at the start would return immediately; mid-buffer still measures
    // scan throughput while exercising the early exit.
    let mid_nul = Input {
        name: "mid-NUL",
        buffers: no_nul
            .buffers
            .iter()
            .map(|(size, text)| {
                let mut binary = text.clone();
                binary[text.len() / 2] = 0;
                (*size, binary)
            })
            .collect(),
    };

    println!("Time per 256 MiB scanned (best of up to 40 runs).");
    println!();
    println!(
        "{:<14}  {:<7}  {:>6} {:>10} {:>10} {:>8}",
        "function", "input", "buffer", "old", "new", "speedup"
    );

    table(
        "is_binary",
        &no_nul,
        |d| d.contains(&0) as usize,
        |d| bytescan::is_binary(d) as usize,
    );
    table(
        "is_binary",
        &mid_nul,
        |d| d.contains(&0) as usize,
        |d| bytescan::is_binary(d) as usize,
    );
    table(
        "count_newlines",
        &no_nul,
        |d| d.split(|c| c == &b'\n').count(),
        bytescan::count_newlines,
    );
}
