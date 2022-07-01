/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clidispatch::dispatch;

mod buildinfo;
#[cfg(all(feature = "with_chg", not(windows)))]
mod chg;
#[cfg(all(feature = "with_chg", not(windows)))]
use chg::maybe_call_chg;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::disable_standard_handle_inheritability;
#[cfg(windows)]
use windows::is_edenfs_stopped;

#[cfg_attr(fbcode_build, fbinit::main)]
fn main() {
    let mut full_args = match dispatch::args() {
        Ok(args) => args,
        Err(_) => {
            eprintln!("abort: cannot decode command line arguments");
            std::process::exit(255);
        }
    };

    // On Windows we need to check if the repository is backed by EdenFS and abort if it is not
    // running to avoid writing any files that may bring the repository into a bad state.
    #[cfg(windows)]
    {
        if let Ok(cwd) = std::env::current_dir() {
            if is_edenfs_stopped(&cwd) {
                eprintln!(
                    "error: repository is not mounted. Check if EdenFS is running and ensure repository is mounted.\nhint: try `edenfsctl doctor`."
                );
                std::process::exit(255);
            }
        }
    }

    match full_args.get(0).map(AsRef::as_ref) {
        Some("buildinfo") => {
            // This code path keeps buildinfo-related symbols alive.
            #[cfg(feature = "buildinfo")]
            unsafe {
                buildinfo::print_buildinfo();
            }

            #[cfg(not(feature = "buildinfo"))]
            {
                eprintln!("buildinfo not compiled in!");
            }

            return;
        }
        Some(name) => {
            if name.ends_with("python") || name.ends_with("python3") {
                // Translate to the "debugpython" command.
                // ex. "python foo.py" => "hg debugpython -- foo.py"
                let debugpython_args = vec!["hg", "debugpython", "--"]
                    .into_iter()
                    .map(ToString::to_string)
                    .chain(full_args.into_iter().skip(1))
                    .collect::<Vec<String>>();
                full_args = debugpython_args;
            }
        }
        _ => {}
    }

    #[cfg(all(feature = "with_chg", not(windows)))]
    maybe_call_chg(&full_args);

    #[cfg(windows)]
    disable_standard_handle_inheritability().unwrap();

    #[cfg(windows)]
    windows::enable_vt_processing().unwrap();

    let mut io = clidispatch::io::IO::stdio();
    io.set_main();
    let mut code = hgcommands::run_command(full_args, &mut io);
    if io.flush().is_err() {
        if code == 0 {
            code = 255;
        }
    }
    drop(io);
    std::process::exit(code as i32);
}
