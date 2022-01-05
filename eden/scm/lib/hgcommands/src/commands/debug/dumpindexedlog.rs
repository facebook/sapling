/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;
use std::path::Path;

use super::ConfigSet;
use super::DebugArgsOpts;
use super::Result;
use super::IO;

pub fn run(opts: DebugArgsOpts, io: &IO, _config: ConfigSet) -> Result<u8> {
    let mut ferr = io.error();
    for path in opts.args {
        let _ = IO::write(&io, format!("{}\n", path));
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
            io.write_err(format!("Path {:?} is not a file or directory.\n\n", path))?;
        }
    }
    Ok(0)
}

pub fn name() -> &'static str {
    "debugdumpindexedlog|debugindexedlogdump"
}

pub fn doc() -> &'static str {
    "dump indexedlog data"
}
