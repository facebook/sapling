/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Binary to probe for offsets and output Rust constants.
//! Writes a Rust source file with OFFSET_IP and OFFSET_SP constants.

fn main() {
    let (ip, sp) = match backtrace_python_offset_probe::get_offsets() {
        Some((ip, sp)) => (Some(ip), Some(sp)),
        None => (None, None),
    };

    #[allow(clippy::print_literal)]
    println!(
        r#"/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// @{}enerated by offset-codegen. Do not edit.

/// IP offset within Sapling_PyEvalFrame where the PyFrame can be read.
pub const OFFSET_IP: Option<usize> = {:?};

/// SP offset within Sapling_PyEvalFrame to read the PyFrame pointer.
pub const OFFSET_SP: Option<usize> = {:?};
"#,
        "g", ip, sp,
    );
}
