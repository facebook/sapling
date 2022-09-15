/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use clidispatch::ReqCtx;
use cliparser::define_flags;
use revsets::utils::resolve_single;

use super::Repo;
use super::Result;

define_flags! {
    pub struct DebugRevsetOpts {
        #[arg]
        rev: String,
    }
}

pub fn run(ctx: ReqCtx<DebugRevsetOpts>, repo: &mut Repo) -> Result<u8> {
    let changelog = repo.dag_commits()?;
    let id_map = changelog.read().id_map_snapshot()?;
    let resolved_revset = resolve_single(&ctx.opts.rev, id_map.as_ref())?;

    write!(ctx.io().output(), "{}\n", resolved_revset.to_hex())?;

    Ok(0)
}

pub fn aliases() -> &'static str {
    "debugrevset"
}

pub fn doc() -> &'static str {
    "resolves a single revset and outputs its commit hash"
}

pub fn synopsis() -> Option<&'static str> {
    None
}
