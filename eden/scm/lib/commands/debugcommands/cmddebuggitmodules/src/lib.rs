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
use repo::Repo;
use workingcopy::workingcopy::WorkingCopy;

pub fn run(ctx: ReqCtx<NoOpts>, _repo: &Repo, wc: &WorkingCopy) -> Result<u8> {
    let gitmodules = wc.parse_submodule_config()?;

    for m in gitmodules {
        write!(ctx.io().output(), "{}", m)?;
    }

    Ok(0)
}

pub fn aliases() -> &'static str {
    "debuggitmodules"
}

pub fn doc() -> &'static str {
    "list git submodules in the current working directory"
}

pub fn synopsis() -> Option<&'static str> {
    None
}

pub fn enable_cas() -> bool {
    false
}
