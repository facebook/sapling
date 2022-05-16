/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::ConfigSet;
use super::DebugArgsOpts;
use super::Result;
use super::IO;

pub fn run(opts: DebugArgsOpts, io: &IO, _config: ConfigSet) -> Result<u8> {
    match io.write(format!("{:?}\n", opts.args)) {
        Ok(_) => Ok(0),
        Err(_) => Ok(255),
    }
}

pub fn name() -> &'static str {
    "debug-args"
}

pub fn doc() -> &'static str {
    "print arguments received"
}

pub fn synopsis() -> Option<&'static str> {
    None
}
