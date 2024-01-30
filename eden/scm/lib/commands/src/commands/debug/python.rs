/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clidispatch::errors;
use clidispatch::ReqCtx;

use super::define_flags;
use super::ConfigSet;
use super::Result;

define_flags! {
    pub struct DebugPythonOpts {
        #[args]
        args: Vec<String>,
    }
}

pub fn run(_ctx: ReqCtx<DebugPythonOpts>, _config: &mut ConfigSet) -> Result<u8> {
    let e = errors::Abort("wrong debugpython code path used".into());
    Err(e.into())
}

pub fn aliases() -> &'static str {
    "debugpython|debugpy"
}

pub fn doc() -> &'static str {
    "run python interpreter"
}

pub fn synopsis() -> Option<&'static str> {
    None
}
