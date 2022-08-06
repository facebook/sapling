/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clidispatch::errors;

use super::define_flags;
use super::ConfigSet;
use super::Result;
use super::IO;

define_flags! {
    pub struct DebugPythonOpts {
        #[args]
        args: Vec<String>,
    }
}

pub fn run(_opts: DebugPythonOpts, _io: &IO, _config: &mut ConfigSet) -> Result<u8> {
    let e = errors::Abort("wrong debugpython code path used".into());
    Err(e.into())
}

pub fn name() -> &'static str {
    "debugpython|debugpy"
}

pub fn doc() -> &'static str {
    "run python interpreter"
}

pub fn synopsis() -> Option<&'static str> {
    None
}
