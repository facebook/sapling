/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use clidispatch::ReqCtx;
use cmdutil::Config;
use cmdutil::NoOpts;
use cmdutil::Result;

pub fn run(_ctx: ReqCtx<NoOpts>, _config: &Arc<dyn Config>) -> Result<u8> {
    Ok(0)
}

pub fn aliases() -> &'static str {
    "debugtestcommand|legacy:debugoldtestcommand|debugothertestcommand"
}

pub fn doc() -> &'static str {
    "basic Rust command with some aliases"
}

pub fn synopsis() -> Option<&'static str> {
    None
}
