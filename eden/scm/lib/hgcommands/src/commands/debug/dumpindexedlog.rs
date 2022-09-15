/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;
use std::path::Path;

use clidispatch::ReqCtx;

use super::ConfigSet;
use super::DebugArgsOpts;
use super::Result;

pub fn run(ctx: ReqCtx<DebugArgsOpts>, _config: &mut ConfigSet) -> Result<u8> {
    let mut ferr = ctx.io().error();
    let mut fout = ctx.io().output();
    for path in ctx.opts.args {
        let _ = write!(fout, "{}\n", path);
        let path = Path::new(&path);
        if let Ok(meta) = indexedlog::log::LogMetadata::read_file(path) {
            write!(ferr, "Metadata File {:?}\n{:?}\n", path, meta)?;
        } else if path.is_dir() {
            // Treate it as Log.
            let log = indexedlog::log::Log::open(path, Vec::new())?;
            write!(ferr, "Log Directory {:?}:\n{:#?}\n", path, log)?;
        } else if path.is_file() {
            // Treate it as Index.
            let idx = indexedlog::index::OpenOptions::new().open(path)?;
            write!(ferr, "Index File {:?}\n{:?}\n", path, idx)?;
        } else {
            write!(ferr, "Path {:?} is not a file or directory.\n\n", path)?;
        }
    }
    Ok(0)
}

pub fn aliases() -> &'static str {
    "debugdumpindexedlog|debugindexedlogdump"
}

pub fn doc() -> &'static str {
    "dump indexedlog data"
}

pub fn synopsis() -> Option<&'static str> {
    None
}
