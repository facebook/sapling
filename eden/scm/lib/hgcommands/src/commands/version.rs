/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::NoOpts;
use super::Result;
use super::IO;

pub fn run(_opts: NoOpts, io: &mut IO) -> Result<u8> {
    io.write(format!("EdenSCM {}\n", ::version::VERSION))?;
    Ok(0)
}

pub fn name() -> &'static str {
    "version|vers|versi|versio"
}

pub fn doc() -> &'static str {
    "output version and copyright information"
}
