/*
 * Copyright (c) Facebook, Inc. and its affiliates.
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
}
