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
use identity::sniff_roots;
use repo::repo::Repo;

pub fn run(ctx: ReqCtx<NoOpts>, repo: &Repo) -> Result<u8> {
    let result = sniff_roots(repo.path())?;
    for (path, _ident) in result {
        write!(ctx.io().output(), "{}\n", path.to_string_lossy())?;
    }

    Ok(0)
}

pub fn aliases() -> &'static str {
    "debugroots"
}

pub fn doc() -> &'static str {
    r#"
    List all the repo roots recursively up to the system root. 
    Useful when inside nested submodules to identify parent repos.
    Returns 0 on success."#
}

pub fn synopsis() -> Option<&'static str> {
    None
}

pub fn enable_cas() -> bool {
    false
}
