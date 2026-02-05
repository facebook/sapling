/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Build script for backtrace-python.
//!
//! This probes the offsets needed to extract Python frames from native stack
//! traces and passes them to the compiler via environment variables.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    if let Some(offsets) = backtrace_python_offset_probe::get_offsets() {
        println!("cargo::rustc-env=BACKTRACE_PYTHON_OFFSET_IP={}", offsets.0);
        println!("cargo::rustc-env=BACKTRACE_PYTHON_OFFSET_SP={}", offsets.1);
        eprintln!("Got offsets: {offsets:?}");
    } else {
        eprintln!("No offset");
    }
}
