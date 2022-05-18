/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::ConfigSet;
use super::NoOpts;
use super::Result;
use super::IO;

pub fn run(_opts: NoOpts, io: &IO, _config: &mut ConfigSet) -> Result<u8> {
    io.write(format!("EdenSCM {}\n", ::version::VERSION))?;
    Ok(0)
}

pub fn name() -> &'static str {
    "version|vers|versi|versio"
}

pub fn doc() -> &'static str {
    "output version and copyright information"
}

pub fn synopsis() -> Option<&'static str> {
    None
}
