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
    #[cfg(target_os = "linux")]
    {
        parse_proc_maps().unwrap_or_default()
    }
    #[cfg(target_os = "macos")]
    {
        parse_dyld_images()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Default::default()
    }
}

/// Parse /proc/self/maps to find libpython and .so ranges
#[cfg(target_os = "linux")]
fn parse_proc_maps() -> Result<Vec<Range>, std::io::Error> {
    let contents = std::fs::read_to_string("/proc/self/maps")?;
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

/// Parse dyld loaded images to find libpython and .dylib/.so ranges
#[cfg(target_os = "macos")]
// libc exposes deprecated dyld APIs and suggests mach2, but mach2 doesn't
// provide the 64-bit Mach-O structs/constants we need.
#[allow(deprecated)]
fn parse_dyld_images() -> Vec<Range> {
    use std::ffi::CStr;

    let mut ranges = Vec::new();
    let count = unsafe { libc::_dyld_image_count() };

    for i in 0..count {
        let header = unsafe { libc::_dyld_get_image_header(i) };
        if header.is_null() {
            continue;
        }

        let name_ptr = unsafe { libc::_dyld_get_image_name(i) };
        if name_ptr.is_null() {
            continue;
        }

        let name = match unsafe { CStr::from_ptr(name_ptr) }.to_str() {
            Ok(s) => s,
            Err(_) => continue,
        };

        if !is_python_library_path(name) {
            continue;
        }

        let slide = unsafe { libc::_dyld_get_image_vmaddr_slide(i) } as isize;

        // Parse Mach-O header to find __TEXT segment
        let header_ref = unsafe { &*header };
        if header_ref.magic != libc::MH_MAGIC_64 {
            continue;
        }

        // Walk through load commands
        let header_64 = header as *const libc::mach_header_64;
        let header_64_ref = unsafe { &*header_64 };
        let mut cmd_ptr =
            unsafe { (header as *const u8).add(std::mem::size_of::<libc::mach_header_64>()) };

        for _ in 0..header_64_ref.ncmds {
            let load_cmd = unsafe { &*(cmd_ptr as *const libc::load_command) };

            if load_cmd.cmd == libc::LC_SEGMENT_64 {
                let seg_cmd = unsafe { &*(cmd_ptr as *const libc::segment_command_64) };

                // Check if this is the __TEXT segment (executable code)
                let segname_bytes: &[u8] = unsafe {
                    std::slice::from_raw_parts(seg_cmd.segname.as_ptr() as *const u8, 16)
                };
                let segname = CStr::from_bytes_until_nul(segname_bytes)
                    .ok()
                    .map(CStr::to_bytes);
                if segname == Some(b"__TEXT".as_slice()) {
                    // Apply ASLR slide to get the actual runtime address
                    let start = (seg_cmd.vmaddr as isize + slide) as usize;
                    let end = start + seg_cmd.vmsize as usize;
                    ranges.push(Range { start, end });
                }
            }

            cmd_ptr = unsafe { cmd_ptr.add(load_cmd.cmdsize as usize) };
        }
    }

    ranges.sort();
    ranges
}

/// Check if a pathname is a Python library or extension
fn is_python_library_path(path: &str) -> bool {
    // Linux examples:
    // /usr/lib/libpython3.10.so.1.0
    // /usr/lib64/python3.11/lib-dynload/math.cpython-311-x86_64-linux-gnu.so
    // ~/.local/lib/python3.11/site-packages/numpy/random/_generator.cpython-311-x86_64-linux-gnu.so
    //
    // macOS examples:
    // /usr/local/Cellar/python@3.11/3.11.4/Frameworks/Python.framework/Versions/3.11/Python
    // /Library/Frameworks/Python.framework/Versions/3.11/lib/python3.11/lib-dynload/_json.cpython-311-darwin.so
    // /opt/homebrew/lib/python3.11/site-packages/numpy/.dylibs/libopenblas64_.0.dylib
    path.contains("/libpython") || path.contains("/python") || path.contains("/Python.framework/")
}
