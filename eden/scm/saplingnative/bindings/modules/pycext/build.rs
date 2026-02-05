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

    let root_dir = Path::new("../../../../../../");
    let cext_dir = Path::new("../../../../sapling/cext/");
    let mut config = PythonSysConfig::new();

    let mut c = cc::Build::new();
    c.files([
        cext_dir.join("../bdiff.c"),
        cext_dir.join("../mpatch.c"),
        cext_dir.join("bdiff.c"),
        cext_dir.join("mpatch.c"),
        cext_dir.join("osutil.c"),
        cext_dir.join("charencode.c"),
        cext_dir.join("manifest.c"),
        cext_dir.join("revlog.c"),
        cext_dir.join("parsers.c"),
        cext_dir.join("../ext/extlib/pywatchman/bser.c"),
    ])
    .include(root_dir)
    .define("HAVE_LINUX_STATFS", "1")
    .define("_GNU_SOURCE", "1")
    .warnings(false)
    .warnings_into_errors(false);
    if !cfg!(windows) {
        c.flag("-std=c99").flag("-Wno-deprecated-declarations");
    }
    config.add_python_flags(&mut c);
    c.compile("cextmodules");

    let mut c = cc::Build::new();
    c.cpp(true)
        .file(cext_dir.join("../ext/extlib/traceprofimpl.cpp"));
    if !cfg!(windows) {
        c.flag("-std=c++11").flag("-Wno-unused-function");
    }
    if cfg!(target_os = "macos") {
        c.flag("-stdlib=libc++");
    }
    config.add_python_flags(&mut c);
    c.compile("traceprofimpl");
}
