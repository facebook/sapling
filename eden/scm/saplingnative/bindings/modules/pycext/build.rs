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

    #[cfg(windows)]
    {
        let mut c = cc::Build::new();
        let curses_dir = cext_dir.join("../../lib/third-party/windows-curses");
        let files = [
            "PDCurses/wincon/pdcclip.c",
            "PDCurses/wincon/pdcgetsc.c",
            "PDCurses/wincon/pdckbd.c",
            "PDCurses/wincon/pdcscrn.c",
            "PDCurses/wincon/pdcsetsc.c",
            "PDCurses/wincon/pdcutil.c",
            "PDCurses/wincon/pdcdisp.c",
            "PDCurses/pdcurses/addch.c",
            "PDCurses/pdcurses/addchstr.c",
            "PDCurses/pdcurses/addstr.c",
            "PDCurses/pdcurses/attr.c",
            "PDCurses/pdcurses/beep.c",
            "PDCurses/pdcurses/bkgd.c",
            "PDCurses/pdcurses/border.c",
            "PDCurses/pdcurses/clear.c",
            "PDCurses/pdcurses/color.c",
            "PDCurses/pdcurses/debug.c",
            "PDCurses/pdcurses/delch.c",
            "PDCurses/pdcurses/deleteln.c",
            "PDCurses/pdcurses/getch.c",
            "PDCurses/pdcurses/getstr.c",
            "PDCurses/pdcurses/getyx.c",
            "PDCurses/pdcurses/inch.c",
            "PDCurses/pdcurses/inchstr.c",
            "PDCurses/pdcurses/initscr.c",
            "PDCurses/pdcurses/inopts.c",
            "PDCurses/pdcurses/insch.c",
            "PDCurses/pdcurses/insstr.c",
            "PDCurses/pdcurses/instr.c",
            "PDCurses/pdcurses/kernel.c",
            "PDCurses/pdcurses/keyname.c",
            "PDCurses/pdcurses/mouse.c",
            "PDCurses/pdcurses/move.c",
            "PDCurses/pdcurses/outopts.c",
            "PDCurses/pdcurses/overlay.c",
            "PDCurses/pdcurses/pad.c",
            "PDCurses/pdcurses/panel.c",
            "PDCurses/pdcurses/printw.c",
            "PDCurses/pdcurses/refresh.c",
            "PDCurses/pdcurses/scanw.c",
            "PDCurses/pdcurses/scr_dump.c",
            "PDCurses/pdcurses/scroll.c",
            "PDCurses/pdcurses/slk.c",
            "PDCurses/pdcurses/termattr.c",
            "PDCurses/pdcurses/touch.c",
            "PDCurses/pdcurses/util.c",
            "PDCurses/pdcurses/window.c",
            "_curses_panel.c",
            "_cursesmodule.c",
            "terminfo.c",
        ];
        c.files(files.iter().map(|f| curses_dir.join(f)).collect::<Vec<_>>())
            .include(&curses_dir)
            .include(curses_dir.join("PDCurses"))
            .define("PDC_WIDE", "")
            .define("HAVE_NCURSESW", "")
            .define("HAVE_TERM_H", "")
            .define("HAVE_CURSES_RESIZE_TERM", "")
            .define("HAVE_CURSES_TYPEAHEAD", "")
            .define("HAVE_CURSES_HAS_KEY", "")
            .define("HAVE_CURSES_FILTER", "")
            .define("HAVE_CURSES_WCHGAT", "")
            .define("HAVE_CURSES_USE_ENV", "")
            .define("HAVE_CURSES_IMMEDOK", "")
            .define("HAVE_CURSES_SYNCOK", "")
            .define("WINDOW_HAS_FLAGS", "")
            .define("_ISPAD", "0x10");
        config.add_python_flags(&mut c);
        c.compile("windows_curses");
    }
}
