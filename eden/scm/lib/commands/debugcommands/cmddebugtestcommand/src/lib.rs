/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use clidispatch::abort;
use clidispatch::ReqCtx;
use cmdutil::define_flags;
use cmdutil::Repo;
use cmdutil::Result;

define_flags! {
    pub struct TestCommandOpts {
        /// set result code
        result: i64 = 0,

        /// force command to abort
        abort: bool = false,

        /// print this back out, followed by newline
        echo: Option<String>,

        #[args]
        args: Vec<String>,
    }
}

pub fn run(ctx: ReqCtx<TestCommandOpts>, _repo: Option<&mut Repo>) -> Result<u8> {
    if ctx.opts.abort {
        abort!("aborting");
    }

    if let Some(output) = &ctx.opts.echo {
        writeln!(ctx.io().output(), "{output}")?;
    }

    Ok(ctx.opts.result as u8)
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
