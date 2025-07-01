/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let lib_rs = Path::new(&env::var("OUT_DIR").unwrap()).join("lib.rs");
    fs::copy("tests/fixtures/lib.rs", lib_rs).expect("Failed copying fixtures");
}
