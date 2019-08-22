// Copyright Facebook, Inc. 2018

use clidispatch::{dispatch, errors};
use hgcommands::{commands, HgPython};
use std::env;

mod buildinfo;
#[cfg(feature = "with_chg")]
mod chg;
#[cfg(feature = "with_chg")]
use chg::maybe_call_chg;

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
    let args = match dispatch::args() {
        Ok(args) => args,
        Err(_) => {
            eprintln!("abort: cannot decode command line arguments");
            std::process::exit(255);
        }
    };

    #[cfg(feature = "buildinfo")]
    {
        // This code path keeps buildinfo-related symbols alive.
        if let Some(arg0) = args.get(0) {
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
    let table = commands::table();
    let args: Vec<String> = args.into_iter().skip(1).collect();
    let mut io = clidispatch::io::IO::stdio();

    match dispatch::dispatch(&table, args, &mut io) {
        Ok(ret) => std::process::exit(ret as i32),
        Err(err) => {
            let should_fallback = if err.downcast_ref::<errors::FallbackToPython>().is_some() {
                true
            } else if err.downcast_ref::<errors::UnknownCommand>().is_some() {
                // XXX: Right now the Rust command table does not have all Python
                // commands. Therefore Rust "UnknownCommand" needs a fallback.
                //
                // Ideally the Rust command table has Python command information and
                // there is no fallback path (ex. all commands are in Rust, and the
                // Rust implementation might just call into Python cmdutil utilities).
                true
            } else {
                false
            };

            if !should_fallback {
                errors::print_error(&err, &mut io);
                std::process::exit(255);
            }

            // Change the current dir back to the original so it is not surprising to the Python
            // code.
            env::set_current_dir(cwd).ok();

            #[cfg(feature = "with_chg")]
            maybe_call_chg();

            call_embedded_python();
        }
    }
}
