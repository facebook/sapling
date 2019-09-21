// Copyright (c) Facebook, Inc. and its affiliates.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2.

use crate::{commands, HgPython};
use clidispatch::{dispatch, errors};
use std::env;
use std::time::SystemTime;

/// Run a Rust or Python command.
///
/// Have side effect on `io` and return the command exit code.
pub fn run_command(args: Vec<String>, io: &mut clidispatch::io::IO) -> i32 {
    let now = SystemTime::now();

    // This is intended to be "process start". "exec/hgmain" seems to be
    // a better place for it. However, chg makes it tricky. Because if hgmain
    // decides to use chg, then there is no way to figure out which `blackbox`
    // to write to, because the repo initialization logic happened in another
    // process (a forked chg server).
    //
    // Having "run_command" here will make it logged by the forked chg server,
    // which is a bit more desiable. Since run_command is very close to process
    // start, it should reflect the duration of the command relatively
    // accurately, at least for non-chg cases.
    log_start(args.clone());

    let cwd = env::current_dir().unwrap();
    let table = commands::table();

    let exit_code = match dispatch::dispatch(&table, args[1..].to_vec(), io) {
        Ok(ret) => ret as i32,
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
                errors::print_error(&err, io);
                return 255;
            }

            // Change the current dir back to the original so it is not surprising to the Python
            // code.
            let _ = env::set_current_dir(cwd);

            HgPython::new(args.clone()).run_hg(args, io)
        }
    };

    log_end(exit_code as u8, now);

    exit_code
}

fn log_start(args: Vec<String>) {
    let (uid, pid, nice) = if is_inside_test() {
        (0, 0, 0)
    } else {
        #[cfg(unix)]
        unsafe {
            (
                libc::getuid() as u32,
                libc::getpid() as u32,
                libc::nice(0) as i32,
            )
        }

        #[cfg(not(unix))]
        unsafe {
            // uid and nice are not aviailable on Windows.
            (0, libc::getpid() as u32, 0)
        }
    };

    blackbox::log(&blackbox::event::Event::Start {
        pid,
        uid,
        nice,
        args,
    });
}

fn log_end(exit_code: u8, now: SystemTime) {
    let inside_test = is_inside_test();
    let duration_ms = if inside_test {
        0
    } else {
        match now.elapsed() {
            Ok(duration) => duration.as_millis() as u64,
            Err(_) => 0,
        }
    };
    let max_rss = if inside_test {
        0
    } else {
        procinfo::max_rss_bytes()
    };

    blackbox::log(&blackbox::event::Event::Finish {
        exit_code,
        max_rss,
        duration_ms,
    });
}

fn is_inside_test() -> bool {
    std::env::var_os("TESTTMP").is_some()
}
