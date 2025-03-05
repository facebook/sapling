/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::env;
use std::ffi::OsString;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-env-changed=PYTHON_SYS_EXECUTABLE");

    let python = match env::var_os("PYTHON_SYS_EXECUTABLE") {
        Some(python) => python,
        None => {
            println!("cargo:warning=PYTHON_SYS_EXECUTABLE is recommended at build time");
            OsString::from("python3")
        }
    };
    let manifest_dir = env::var_os("CARGO_MANIFEST_DIR").unwrap();
    let manifest_dir = Path::new(&manifest_dir);
    let sys_path = manifest_dir.parent().unwrap().parent().unwrap();

    let code = codegen::generate_code(&Path::new(&python), Some(sys_path.as_ref()));

    let out = manifest_dir.join("src/compiled.rs");
    std::fs::write(out, code).unwrap();
}
