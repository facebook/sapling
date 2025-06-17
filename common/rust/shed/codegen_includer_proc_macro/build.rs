/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let lib_rs = Path::new(&env::var("OUT_DIR").unwrap()).join("lib.rs");
    fs::copy("tests/fixtures/lib.rs", lib_rs).expect("Failed copying fixtures");
}
