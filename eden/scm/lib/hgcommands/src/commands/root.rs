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

        /// join root with the repo dot dir (e.g. ".sl") (EXPERIMENTAL)
        dotdir: bool,
    }
}

pub fn run(ctx: ReqCtx<RootOpts>, repo: &mut Repo) -> Result<u8> {
    let path = match (ctx.opts.shared, ctx.opts.dotdir) {
        (false, false) => repo.path(),
        (false, true) => repo.dot_hg_path(),
        (true, false) => repo.shared_path(),
        (true, true) => repo.shared_dot_hg_path(),
    };

    ctx.io().write(format!(
        "{}\n",
        util::path::strip_unc_prefix(&path).display()
    ))?;
    Ok(0)
}

pub fn aliases() -> &'static str {
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
