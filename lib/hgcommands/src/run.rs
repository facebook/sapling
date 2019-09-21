// Copyright (c) Facebook, Inc. and its affiliates.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2.

use crate::{commands, HgPython};
use clidispatch::{dispatch, errors};
use std::env;

/// Run a Rust or Python command.
///
/// Have side effect on `io` and return the command exit code.
pub fn run_command(args: Vec<String>, io: &mut clidispatch::io::IO) -> i32 {
    let cwd = env::current_dir().unwrap();
    let table = commands::table();

    match dispatch::dispatch(&table, args[1..].to_vec(), io) {
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
    }
}
