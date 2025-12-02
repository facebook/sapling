/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(unexpected_cfgs)]

use std::str::FromStr;

use clidispatch::ReqCtx;
use cmdutil::define_flags;
use filters::filter::FilterGenerator;
use repo::repo::Repo;
use types::HgId;
use types::RepoPathBuf;

define_flags! {
    pub struct DebugFilterIdOpts {
        /// Revision
        #[short('r')]
        #[argtype("REV")]
        rev: String,


        #[args]
        args: Vec<String>,
    }
}

pub fn run(ctx: ReqCtx<DebugFilterIdOpts>, repo: &Repo) -> cmdutil::Result<u8> {
    let config = repo.config();
    let mut filter_gen =
        FilterGenerator::from_dot_dirs(repo.dot_hg_path(), repo.shared_dot_hg_path(), config)?;
    let paths = ctx
        .opts
        .args
        .into_iter()
        .map(RepoPathBuf::from_string)
        .collect::<Result<Vec<RepoPathBuf>, _>>()?;
    let commit_id = HgId::from_str(&ctx.opts.rev)?;
    let filter = filter_gen.generate_filter_id(commit_id, &paths)?;
    ctx.core.io.write(filter.id()?)?;
    Ok(0)
}

pub fn aliases() -> &'static str {
    "debugfilterid"
}

pub fn doc() -> &'static str {
    r#"
    Prints out a filter id for the filter constructed of the provided
    filter files and commit hash. Useful for EdenFS clone operations that
    must construct a filter id prior to setting up the mount point.
    Returns 0 on success."#
}

pub fn synopsis() -> Option<&'static str> {
    None
}
