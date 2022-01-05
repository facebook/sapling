/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use clidispatch::errors;
use cliparser::define_flags;
use dag::render::render_segment_dag;

use super::Repo;
use super::Result;
use super::IO;

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

pub fn run(opts: GraphOpts, io: &IO, repo: Repo) -> Result<u8> {
    let group = match opts.group.as_ref() {
        "master" => dag::Group::MASTER,
        "non_master" => dag::Group::NON_MASTER,
        _ => return Err(errors::Abort("invalid group".into()).into()),
    };

    let level: dag::Level = match opts.level.try_into() {
        Ok(level) => level,
        _ => return Err(errors::Abort("invalid level".into()).into()),
    };

    let mut out = io.output();
    write!(out, "{}, Level: {}\n", group, level)?;

    let dag = dag::Dag::open(repo.store_path().join("segments/v1"))?;
    render_segment_dag(out, &dag, level, group)?;

    Ok(0)
}

pub fn name() -> &'static str {
    "debugsegmentgraph"
}

pub fn doc() -> &'static str {
    "display segment graph for a given group and level"
}
