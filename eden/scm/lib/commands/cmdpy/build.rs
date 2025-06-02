/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::env;

fn main() {
    #[cfg(target_os = "linux")]
    {
        if is_libpython_static() {
            // Tell cmdpy to configure Python to use relative stdlib.
            println!("cargo:rustc-cfg=static_libpython");
        }
    }
}

#[cfg(target_os = "linux")]
fn is_libpython_static() -> bool {
    use std::ffi::OsString;
    use std::process::Command;

    let python = match env::var_os("PYTHON_SYS_EXECUTABLE") {
        Some(python) => python,
        None => {
            println!("cargo:warning=PYTHON_SYS_EXECUTABLE is recommended at build time");
            OsString::from("python3")
        }
    };
    let out = Command::new(&python)
        .args([
            "-Sc",
            "print(__import__('sysconfig').get_config_var('Py_ENABLE_SHARED'))",
        ])
        .output()
        .expect("Failed to get Py_ENABLE_SHARED from Python");
    out.stdout.starts_with(b"0")
}
