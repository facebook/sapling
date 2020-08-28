/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::define_flags;
use super::Result;
use super::IO;

define_flags! {
    pub struct DebugPythonOpts {
        #[args]
        args: Vec<String>,
    }
}

pub fn run(opts: DebugPythonOpts, io: &mut IO) -> Result<u8> {
    let mut args = opts.args;
    args.insert(0, "hgpython".to_string());
    let mut interp = crate::HgPython::new(&args);
    Ok(interp.run_python(&args, io))
}

pub fn name() -> &'static str {
    "debugpython|debugpy"
}

pub fn doc() -> &'static str {
    "run python interpreter"
}
