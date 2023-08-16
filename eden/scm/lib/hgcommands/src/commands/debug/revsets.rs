/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use clidispatch::ReqCtx;
use cliparser::define_flags;
use workingcopy::workingcopy::WorkingCopy;

use super::Repo;
use super::Result;

define_flags! {
    pub struct DebugRevsetOpts {
        #[arg]
        rev: String,
    }
}

pub fn run(ctx: ReqCtx<DebugRevsetOpts>, repo: &mut Repo, wc: &mut WorkingCopy) -> Result<u8> {
    let resolved_revset = repo.resolve_commit(Some(&wc.treestate().lock()), &ctx.opts.rev)?;

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
