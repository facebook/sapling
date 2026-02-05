/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;

use python_sysconfig::PythonSysConfig;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let c_src = Path::new("src/evalframe.c");
    if let Some(path) = c_src.to_str() {
        println!("cargo:rerun-if-changed={}", path);
    }

    let mut config = PythonSysConfig::new();
    let mut c = cc::Build::new();
    c.files([c_src]).warnings(false).warnings_into_errors(false);
    if !cfg!(windows) {
        c.flag("-std=c99").flag("-Wno-deprecated-declarations");
    }
    config.add_python_flags(&mut c);
    c.compile("evalframe");
}
