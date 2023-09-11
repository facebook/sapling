/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::process::Command;

struct PythonSysConfig {
    cflags: String,
    ldflags: String,
    include_dir: String,
}

impl PythonSysConfig {
    fn load() -> Self {
        println!("cargo:rerun-if-env-changed=PYTHON_SYS_EXECUTABLE");
        let python = std::env::var("PYTHON_SYS_EXECUTABLE")
            .expect("Building bindings.cext requires PYTHON_SYS_EXECUTABLE");
        let script = concat!(
            "import sysconfig;",
            "print((sysconfig.get_config_var('CFLAGS') or '').strip());",
            "print((sysconfig.get_config_var('LDFLAGS') or '').strip());",
            "print(sysconfig.get_paths()['include'].strip());",
        );

        let out = Command::new(&python)
            .arg("-c")
            .arg(script)
            .output()
            .expect("Failed to get CFLAGS from Python");
        let out_str = String::from_utf8_lossy(&out.stdout);
        let lines: Vec<&str> = out_str.lines().collect();
        if lines.len() < 3 {
            println!(
                "cargo:warning=Python sysconfig output is imcomplete: {:?} Python: {:?}",
                out_str, python
            );
        }
        Self {
            cflags: lines[0].to_string(),
            ldflags: lines[1].to_string(),
            include_dir: lines[2].to_string(),
        }
    }

    fn add_python_flags(&self, c: &mut cc::Build) {
        for flag in self.cflags.split_whitespace().filter(|s| pick_flag(s)) {
            c.flag(flag);
        }
        for flag in self.ldflags.split_whitespace().filter(|s| pick_flag(s)) {
            c.flag(flag);
        }
        c.include(&self.include_dir);
    }
}

// Ignore flags that are annoying for our code.
fn pick_flag(flag: &str) -> bool {
    return !flag.starts_with("-W");
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let root_dir = Path::new("../../../../../../");
    let cext_dir = Path::new("../../../../edenscm/cext/");
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
    config.add_python_flags(&mut c);
    c.compile("traceprofimpl");
}
