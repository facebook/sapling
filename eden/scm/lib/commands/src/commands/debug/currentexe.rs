/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clidispatch::ReqCtx;

use super::ConfigSet;
use super::NoOpts;
use super::Result;

pub fn run(ctx: ReqCtx<NoOpts>, _config: &mut ConfigSet) -> Result<u8> {
    let path = std::env::current_exe()?;
    let out = format!("{}\n", path.display());
    ctx.io().write(out)?;
    Ok(0)
}

pub fn aliases() -> &'static str {
    "debugcurrentexe"
}

pub fn doc() -> &'static str {
    "show the path to the main executable"
}

pub fn synopsis() -> Option<&'static str> {
    None
}
