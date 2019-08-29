// Copyright Facebook, Inc. 2018

use clidispatch::dispatch;

mod buildinfo;
#[cfg(feature = "with_chg")]
mod chg;
#[cfg(feature = "with_chg")]
use chg::maybe_call_chg;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::disable_standard_handle_inheritability;

fn main() {
    let full_args = match dispatch::args() {
        Ok(args) => args,
        Err(_) => {
            eprintln!("abort: cannot decode command line arguments");
            std::process::exit(255);
        }
    };

    if let Some(cmd) = full_args.get(1) {
        if cmd.ends_with("buildinfo") {
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
    }

    #[cfg(feature = "with_chg")]
    maybe_call_chg();

    #[cfg(windows)]
    disable_standard_handle_inheritability().unwrap();

    let mut io = clidispatch::io::IO::stdio();
    let code = hgcommands::run_command(full_args, &mut io);
    std::process::exit(code as i32);
}
