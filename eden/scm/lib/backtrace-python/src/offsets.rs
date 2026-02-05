/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Offsets for extracting Python frames from native stack traces.
//!
//! For Cargo builds, these are read from environment variables set by build.rs.
//! For Buck builds, this file is replaced by a generated version with constants.

/// IP offset within Sapling_PyEvalFrame where the PyFrame can be read.
pub const OFFSET_IP: Option<usize> = match option_env!("BACKTRACE_PYTHON_OFFSET_IP") {
    Some(s) if !s.is_empty() => Some(parse_usize(s)),
    _ => None,
};

/// SP offset to read the PyFrame pointer.
pub const OFFSET_SP: Option<usize> = match option_env!("BACKTRACE_PYTHON_OFFSET_SP") {
    Some(s) if !s.is_empty() => Some(parse_usize(s)),
    _ => None,
};

/// Parse a usize from a string at compile time.
const fn parse_usize(s: &str) -> usize {
    let bytes = s.as_bytes();
    let mut result: usize = 0;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b >= b'0' && b <= b'9' {
            result = result * 10 + (b - b'0') as usize;
        }
        i += 1;
    }
    result
}
