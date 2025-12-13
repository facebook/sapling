/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Filter to identify Python interpreter and extension frames.
//! Used to exclude libpython frames from stack traces while keeping
//! resolved Python frames.

use std::cmp::Ordering;
use std::fs;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Range {
    pub start: usize,
    pub end: usize,
}

/// Global storage for Python library ranges
static PYTHON_RANGES: OnceLock<Vec<Range>> = OnceLock::new();

/// Initialize Python ranges so `is_python_frame` might report `true`.
pub fn init() {
    PYTHON_RANGES.get_or_init(get_python_ranges);
}

/// Check if a program counter (PC) is within Python library ranges
/// This function should be async-signal-safe.
pub fn is_python_frame(pc: usize) -> bool {
    // Do not initialize PYTHON_RANGES here for async-signal-safety.
    let ranges = match PYTHON_RANGES.get() {
        None => return false,
        Some(ranges) => ranges,
    };

    // Binary search since ranges are sorted
    ranges
        .binary_search_by(|range| {
            if pc < range.start {
                Ordering::Greater
            } else if pc >= range.end {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        })
        .is_ok()
}

fn get_python_ranges() -> Vec<Range> {
    if cfg!(target_os = "linux") {
        parse_proc_maps().unwrap_or_default()
    } else {
        // NOTE: For macOS, consider implementing using vmmap APIs.
        Default::default()
    }
}

/// Parse /proc/self/maps to find libpython and .so ranges
fn parse_proc_maps() -> Result<Vec<Range>, std::io::Error> {
    let contents = fs::read_to_string("/proc/self/maps")?;
    let mut ranges = Vec::with_capacity(contents.lines().count());

    for line in contents.lines() {
        // Format: address perms offset dev inode pathname
        // Example: 7f1234567000-7f1234789000 r-xp 00000000 08:01 123456 /usr/lib/libpython3.10.so.1.0
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 6 {
            continue;
        }

        let addr_range = parts[0];
        let perms = parts[1];
        let pathname = parts[5];

        // Only include executable segments
        if !perms.contains('x') {
            continue;
        }
        if !is_python_library_path(pathname) {
            continue;
        }

        if let Some((start_str, end_str)) = addr_range.split_once('-') {
            if let (Ok(start), Ok(end)) = (
                usize::from_str_radix(start_str, 16),
                usize::from_str_radix(end_str, 16),
            ) {
                ranges.push(Range { start, end });
            }
        }
    }

    ranges.sort();
    Ok(ranges)
}

/// Check if a pathname is a Python library or extension
fn is_python_library_path(path: &str) -> bool {
    // Examples:
    // /usr/lib/libpython3.10.so.1.0
    // /usr/lib64/python3.11/lib-dynload/math.cpython-311-x86_64-linux-gnu.so
    // ~/.local/lib/python3.11/site-packages/numpy/random/_generator.cpython-311-x86_64-linux-gnu.so
    path.contains("/libpython") || path.contains("/python")
}
