/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use clidispatch::ReqCtx;
use cmdutil::NoOpts;
use cmdutil::Result;

pub fn run(ctx: ReqCtx<NoOpts>) -> Result<u8> {
    let config = ctx.config();
    let mut stdout = ctx.io().output();
    match network_doctor::Doctor::new().diagnose(config) {
        Ok(()) => {
            write!(stdout, "No network problems detected.\n")?;
            Ok(0)
        }
        Err(d) => {
            write!(stdout, "{}\n\n{}\n", d.treatment(config), d)?;
            Ok(1)
        }
    }
}

pub fn aliases() -> &'static str {
    "debugnetworkdoctor"
}

pub fn doc() -> &'static str {
    "run the (Rust) network doctor"
}

pub fn synopsis() -> Option<&'static str> {
    None
}
