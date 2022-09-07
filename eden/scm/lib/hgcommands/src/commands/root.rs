/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clidispatch::ReqCtx;

use super::define_flags;
use super::Repo;
use super::Result;

define_flags! {
    pub struct RootOpts {
        /// show root of the shared repo
        shared: bool,
    }
}

pub fn run(ctx: ReqCtx<RootOpts>, repo: &mut Repo) -> Result<u8> {
    let path = if ctx.opts.shared {
        repo.shared_path()
    } else {
        repo.path()
    };

    ctx.io().write(format!(
        "{}\n",
        util::path::strip_unc_prefix(&path).display()
    ))?;
    Ok(0)
}

pub fn name() -> &'static str {
    "root"
}

pub fn doc() -> &'static str {
    r#"print the root (top) of the current working directory

    Print the root directory of the current repository.

    Returns 0 on success."#
}

pub fn synopsis() -> Option<&'static str> {
    None
}
