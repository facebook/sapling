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

    #[cfg(target_os = "linux")]
    {
        if is_libpython_static() {
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
