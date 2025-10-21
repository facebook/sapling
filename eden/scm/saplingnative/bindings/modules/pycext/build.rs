/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;

struct PythonSysConfig {
    cflags: String,
    ldflags: String,
    // ex. ~/cpython/Include, or /usr/local/include/python3.10
    include_dir: String,
    // ex. /usr/local/include/python3.10, or empty
    headers: String,
}

impl PythonSysConfig {
    fn load() -> Self {
        let mut sysconfig = python_sysconfig::PythonSysConfig::new();
        Self {
            cflags: sysconfig.cflags(),
            ldflags: sysconfig.ldflags(),
            include_dir: sysconfig.include(),
            headers: sysconfig.headers(),
        }
    }

    fn add_python_flags(&self, c: &mut cc::Build) {
        for flag in self.cflags.split_whitespace().filter(|s| pick_flag(s)) {
            c.flag(flag);
        }
        for flag in self.ldflags.split_whitespace().filter(|s| pick_flag(s)) {
            c.flag(flag);
        }
        if !self.headers.is_empty() {
            c.include(&self.headers);
        }
        if !self.include_dir.is_empty() {
            c.include(&self.include_dir);
        }
    }
}

// Ignore flags that are annoying for our code.
fn pick_flag(flag: &str) -> bool {
    return !flag.starts_with("-W");
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let root_dir = Path::new("../../../../../../");
    let cext_dir = Path::new("../../../../sapling/cext/");
    let config = PythonSysConfig::load();

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
        cext_dir.join("evalframe.c"),
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
