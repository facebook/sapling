/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use clidispatch::errors;
use clidispatch::ReqCtx;
use cliparser::define_flags;
use dag::render::render_segment_dag;

use super::Repo;
use super::Result;

define_flags! {
    pub struct GraphOpts {
        /// segment level (0 is flat)
        #[short('l')]
        level: i64 = 0,

        /// segment group (master|non_master)
        #[short('g')]
        group: String = "master",
    }
}

pub fn run(ctx: ReqCtx<GraphOpts>, repo: &mut Repo) -> Result<u8> {
    let group = match ctx.opts.group.as_ref() {
        "master" => dag::Group::MASTER,
        "non_master" => dag::Group::NON_MASTER,
        _ => return Err(errors::Abort("invalid group".into()).into()),
    };

    let level: dag::Level = match ctx.opts.level.try_into() {
        Ok(level) => level,
        _ => return Err(errors::Abort("invalid level".into()).into()),
    };

    let mut out = ctx.io().output();
    write!(out, "{}, Level: {}\n", group, level)?;

    let dag = dag::Dag::open(repo.store_path().join("segments/v1"))?;
    render_segment_dag(out, &dag, level, group)?;

    Ok(0)
}

pub fn aliases() -> &'static str {
    "debugsegmentgraph"
}

pub fn doc() -> &'static str {
    "display segment graph for a given group and level"
}

pub fn synopsis() -> Option<&'static str> {
    None
}
