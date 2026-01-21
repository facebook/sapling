/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;

use python_sysconfig::PythonSysConfig;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let cext_dir = Path::new("../../sapling/cext/");
    let mut config = PythonSysConfig::new();

    let c_src = cext_dir.join("evalframe.c");
    if let Some(path) = c_src.to_str() {
        println!("cargo:rerun-if-changed={}", path);
    }

    let mut c = cc::Build::new();
    c.files([c_src]).warnings(false).warnings_into_errors(false);
    if !cfg!(windows) {
        c.flag("-std=c99").flag("-Wno-deprecated-declarations");
    }
    config.add_python_flags(&mut c);
    c.compile("evalframe");
}
