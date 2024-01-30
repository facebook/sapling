/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clidispatch::ReqCtx;
use cmdutil::define_flags;
use cmdutil::ConfigSet;
use cmdutil::Result;

define_flags! {
    pub struct DebugArgsOpts {
        #[args]
        args: Vec<String>,
    }
}

pub fn run(ctx: ReqCtx<DebugArgsOpts>, _config: &mut ConfigSet) -> Result<u8> {
    match ctx.io().write(format!("{:?}\n", ctx.opts.args)) {
        Ok(_) => Ok(0),
        Err(_) => Ok(255),
    }
}

pub fn aliases() -> &'static str {
    "debug-args"
}

pub fn doc() -> &'static str {
    "print arguments received"
}

pub fn synopsis() -> Option<&'static str> {
    None
}
