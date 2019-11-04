/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::{commands, HgPython};
use clidispatch::{dispatch, errors};
use std::env;
use std::io;
use std::path::PathBuf;
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

    let cwd = match current_dir(io) {
        Err(e) => {
            let _ = io.write_err(format!("abort: cannot get current directory: {}\n", e));
            return exitcode::IOERR;
        }
        Ok(dir) => dir,
    };
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

    // Sync the blackbox before returning: this exit code is going to be used to process::exit(),
    // so we need to flush now.
    blackbox::sync();

    exit_code
}

/// Similar to `std::env::current_dir`. But does some extra things:
/// - Attempt to autofix issues when running under a typical shell (which
///   sets $PWD), and a directory is deleted and then recreated.
fn current_dir(io: &mut clidispatch::io::IO) -> io::Result<PathBuf> {
    let result = env::current_dir();
    if let Err(ref err) = result {
        match err.kind() {
            io::ErrorKind::NotConnected | io::ErrorKind::NotFound => {
                // For those errors, attempt to fix it by `cd $PWD`.
                // - NotConnected: edenfsctl stop; edenfsctl start
                // - NotFound: rmdir $PWD; mkdir $PWD
                if let Ok(pwd) = env::var("PWD") {
                    if env::set_current_dir(pwd).is_ok() {
                        let _ = io.write_err("(warning: the current directory was recrated, consider running 'cd $PWD' to fix your shell)\n");
                        return env::current_dir();
                    }
                }
            }
            _ => (),
        }
    }
    result
}

fn log_start(args: Vec<String>) {
    let inside_test = is_inside_test();
    let (uid, pid, nice) = if inside_test {
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

    let mut parent_names = Vec::new();
    let mut parent_pids = Vec::new();
    if !inside_test {
        let mut ppid = procinfo::parent_pid(0);
        while ppid != 0 {
            let name = procinfo::exe_name(ppid);
            parent_names.push(name);
            parent_pids.push(ppid);
            ppid = procinfo::parent_pid(ppid);
        }
    }
    blackbox::log(&blackbox::event::Event::ProcessTree {
        names: parent_names,
        pids: parent_pids,
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

// TODO: Replace this with the 'exitcode' crate once it's available.
mod exitcode {
    pub const IOERR: i32 = 74;
}
