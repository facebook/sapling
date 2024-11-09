/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

fn main() {
    cxx_build::bridge("src/lib.rs");
    println!("cargo:rerun-if-changed=src/lib.rs");
}
