// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Run function repetitively and prints out aggregated result.
//!
//! Differences from other benchmark library:
//! - Do not enforce measuring "wall time". It could be changed to "CPU time", "memory usage", etc.
//! - Do not run only the benchmark part repetitively. For example, a benchmark needs some complex
//!   setup that cannot be reused across runs. That setup cost needs to be excluded from benchmark
//!   result cleanly.
//! - Use "min" instead of "average". "average" often makes less sense for wall time.
//! - Minimalism. Without fancy features.

use std::env::args;
use std::fmt::Display;
use std::time::SystemTime;

/// Return the wall time (in seconds) executing the given function.
pub fn elapsed<F: FnMut()>(mut func: F) -> f64 {
    let now = SystemTime::now();
    func();
    let elapsed = now.elapsed().expect("elapsed");
    elapsed.as_secs() as f64 + elapsed.subsec_nanos() as f64 * 1e-9
}

/// Execute a function that returns `f64` for `n` times. Return the min value among those `n`
/// returned values.
pub fn min_run<F: Fn() -> f64>(func: F, n: usize) -> f64 {
    let mut best = ::std::f64::NAN;
    assert!(n > 0);
    for _ in 0..n {
        let v = func();
        if best.is_nan() || v < best {
            best = v
        };
    }
    best
}

/// Run a function for 40 times. Print the best wall time.
///
/// If `std::env::args` (excluding the first item and flags) is not empty, and none of them is a
/// substring of `name`, skip running and return directly.
///
/// Example:
///
/// ```
/// use minibench::*;
/// bench("example", || {
///     // prepare
///     elapsed(|| {
///         // measure
///     })
/// })
/// ```
pub fn bench<F: Fn() -> f64>(name: &str, func: F) {
    // The first arg is the program name. Skip it and flag-like arguments (ex. --bench).
    let args: Vec<String> = args().skip(1).filter(|a| !a.starts_with('-')).collect();
    if args.is_empty() || args.iter().any(|a| name.find(a).is_some()) {
        let best = min_run(func, 40);
        println!("{:30}{:7.3} ms", name, best * 1e3);
    }
}

/// Run a function once. Print its result.
///
/// If `std::env::args` (excluding the first item and flags) is not empty, and none of them is a
/// substring of `name`, skip running and return directly.
pub fn bench_once<D: Display, F: Fn() -> D>(name: &str, func: F) {
    // The first arg is the program name. Skip it and flag-like arguments (ex. --bench).
    let args: Vec<String> = args().skip(1).filter(|a| !a.starts_with('-')).collect();
    if args.is_empty() || args.iter().any(|a| name.find(a).is_some()) {
        println!("{:30}{:10}", name, func());
    }
}
