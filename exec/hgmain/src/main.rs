// Copyright Facebook, Inc. 2018

#[cfg(feature = "with_chg")]
extern crate dirs;
extern crate encoding;
extern crate hgpython;
#[cfg(feature = "with_chg")]
extern crate libc;
use hgpython::HgPython;

mod buildinfo;
#[cfg(feature = "with_chg")]
mod chg;
#[cfg(feature = "with_chg")]
use chg::maybe_call_chg;

mod commands;
use commands::{create_dispatcher, dispatch};

use std::env;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::disable_standard_handle_inheritability;

/// Execute a command, using an embedded interpreter
/// This function does not return
fn call_embedded_python() {
    let code = {
        let hgpython = HgPython::new();
        hgpython.run()
    };
    std::process::exit(code);
}

fn main() {
    #[cfg(feature = "buildinfo")]
    {
        // This code path keeps buildinfo-related symbols alive.
        use std::env;
        if let Some(arg0) = env::args().nth(0) {
            if arg0.ends_with("buildinfo") {
                unsafe {
                    buildinfo::print_buildinfo();
                }
                return;
            }
        }
    }

    #[cfg(windows)]
    disable_standard_handle_inheritability().unwrap();

    let cwd = env::current_dir().unwrap();

    let mut dispatcher = create_dispatcher();

    match dispatch(&mut dispatcher) {
        Ok(ret) => std::process::exit(ret as i32),
        Err(_) => {
            // Change the current dir back to the original so it is not surprising to the Python
            // code.
            env::set_current_dir(cwd).ok();

            #[cfg(feature = "with_chg")]
            maybe_call_chg();

            call_embedded_python();
        }
    }
}
