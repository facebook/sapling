/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

fn main() {
    if cfg!(target_os = "linux") {
        let mut sysconfig = python_sysconfig::PythonSysConfig::new();
        if sysconfig.is_static() {
            // Tell cmdpy to configure Python to use relative stdlib.
            println!("cargo:rustc-cfg=static_libpython");
        }
        let version = sysconfig.version();
        if version >= (3, 12) {
            println!("cargo:rustc-cfg=python_since_3_12");
        }
    }
}
