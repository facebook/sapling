/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clidispatch::ReqCtx;

use super::NoOpts;
use super::Repo;
use super::Result;

pub fn run(_ctx: ReqCtx<NoOpts>, _repo: &mut Repo) -> Result<u8> {
    hg_metrics::increment_counter("test_counter", 1);
    tracing::debug!(target: "test_trace", trace_key="trace-value");
    Ok(0)
}

pub fn aliases() -> &'static str {
    "debugmetrics"
}

pub fn doc() -> &'static str {
    "output test metrics from native command"
}

pub fn synopsis() -> Option<&'static str> {
    None
}
