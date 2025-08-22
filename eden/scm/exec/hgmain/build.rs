/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::env;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if let Some(lib_dirs) = env::var_os("LIB_DIRS") {
        for lib_dir in std::env::split_paths(&lib_dirs) {
            println!("cargo:rustc-link-search={}", lib_dir.display());
            println!(
                "cargo:rerun-if-changed={}",
                lib_dir.join("libchg.a").display()
            );
            #[cfg(feature = "buildinfo")]
            println!(
                "cargo:rerun-if-changed={}",
                lib_dir.join("buildinfo.a").display()
            );
        }
    }

    if cfg!(target_os = "linux") {
        let mut sysconfig = python_sysconfig::PythonSysConfig::new();
        if sysconfig.is_static() {
            // libpython.a typically (ex. built by pypa/manylinux) is not built with
            // `CFLAGS=-fPIC LDFLAGS=-fPIC` and they will fail to link, like:
            //
            //   /usr/bin/ld.gold: error: */libpython3_sys-*.rlib(abstract.o): requires dynamic
            //   R_X86_64_32 reloc against '_PyRuntime' which may overflow at runtime; recompile with
            //   -fPIC
            //
            // Use `no-pie` be compatible with such `libpython.a`. The downside is that the built
            // binary is not PIE (Position Independent Executable) and ASLR (Address Space Layout
            // Randomization) cannot be used.
            println!("cargo:rustc-link-arg=-no-pie");

            // Python modules (.so) want symbols like `PyFloat_Type`.
            // Use `--export-dynamic` to resolve issues like:
            // Traceback (most recent call last):
            //   File "static:sapling", line 62, in run
            //     from . import dispatch
            //   File "static:sapling.dispatch", line 25, in <module>
            //     from . import (
            //   File "static:sapling.alerts", line 9, in <module>
            //     from . import cmdutil, templater
            //   File "static:sapling.cmdutil", line 20, in <module>
            //     import tempfile
            //   File "/opt/python/cp312-cp312/lib/python3.12/tempfile.py", line 45, in <module>
            //     from random import Random as _Random
            //   File "/opt/python/cp312-cp312/lib/python3.12/random.py", line 54, in <module>
            //     from math import log as _log, exp as _exp, pi as _pi, e as _e, ceil as _ceil
            // ImportError: /opt/_internal/cpython-3.12.10/lib/python3.12/lib-dynload/math.cpython-312-x86_64-linux-gnu.so: undefined symbol: PyFloat_Type
            println!("cargo:rustc-link-arg=-Wl,--export-dynamic");
        }
    }

    if !cfg!(windows) {
        use std::path::Path;
        let chg_dir = Path::new("../../contrib/chg");
        let mut c = cc::Build::new();
        c.files([
            chg_dir.join("hgclient.c"),
            chg_dir.join("procutil.c"),
            chg_dir.join("util.c"),
            chg_dir.join("chg.c"),
        ])
        .include(chg_dir)
        .define("_GNU_SOURCE", "1")
        .warnings_into_errors(false)
        .flag("-std=c99");

        // chg uses libc::unistd/getgroups() to check that chg and the
        // sl cli have the same permissions (see D43676809).
        // However, on macOS, getgroups() is limited to NGROUPS_MAX (16) groups by default.
        // We can work around this by defining _DARWIN_UNLIMITED_GETGROUPS
        // see https://opensource.apple.com/source/xnu/xnu-3247.1.106/bsd/man/man2/getgroups.2.auto.html
        if cfg!(target_os = "macos") {
            c.define("_DARWIN_UNLIMITED_GETGROUPS", "1");
        }

        c.compile("chg");
    }
}
