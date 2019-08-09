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
//! - Minimalism. Without fancy features.

use std::env::args;

pub mod measure;
pub use measure::Measure;

/// Measure the best wall clock time.
pub fn elapsed(func: impl FnMut()) -> Result<self::measure::WallClock, String> {
    self::measure::WallClock::measure(func)
}

/// Run a function repeatably. Print the measurement result.
///
/// The actual measurement is dependent on the return value of the function.
/// For example,
/// - [`WallClock::measure`] (or [`elapsed`]) measures the wall clock time,
///   and the function being measured does not need to provide an output.
/// - [`Bytes::measure`] might expect the function to return a [`usize`]
///   in bytes. The function will be only run once.
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
pub fn bench<T: Measure, F: FnMut() -> Result<T, String>>(name: impl ToString, mut func: F) {
    let name = name.to_string();
    // The first arg is the program name. Skip it and flag-like arguments (ex. --bench).
    let args: Vec<String> = args().skip(1).filter(|a| !a.starts_with('-')).collect();
    if args.is_empty() || args.iter().any(|a| name.find(a).is_some()) {
        let mut try_func = || -> Result<T, String> {
            let mut measured = func()?;
            while measured.need_more() {
                measured = measured.merge(func()?);
            }
            Ok(measured)
        };
        let text = match try_func() {
            Ok(measured) => measured.to_string(),
            Err(text) => text,
        };
        println!("{:50}{}", name, text);
    }
}
