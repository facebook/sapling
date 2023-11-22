/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use clidispatch::ReqCtx;
use workingcopy::workingcopy::WorkingCopy;

use super::NoOpts;
use super::Repo;
use super::Result;

pub fn run(ctx: ReqCtx<NoOpts>, _repo: &mut Repo, wc: &mut WorkingCopy) -> Result<u8> {
    let mut io = ctx.io().output();

    let ms = match wc.read_merge_state() {
        Ok(Some(ms)) => ms,
        Ok(None) => {
            writeln!(io, "no merge state found")?;
            return Ok(0);
        }
        Err(err) => match err.downcast::<repostate::UnsupportedMergeRecords>() {
            Ok(bad) => bad.0,
            Err(err) => return Err(err),
        },
    };

    write!(io, "{:?}", ms)?;

    Ok(0)
}

pub fn aliases() -> &'static str {
    "debugmergestate"
}

pub fn doc() -> &'static str {
    "print merge state"
}

pub fn synopsis() -> Option<&'static str> {
    None
}
