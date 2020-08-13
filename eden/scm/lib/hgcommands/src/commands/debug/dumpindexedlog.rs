/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::DebugArgsOpts;
use super::Result;
use super::IO;
use std::path::Path;

pub fn run(opts: DebugArgsOpts, io: &mut IO) -> Result<u8> {
    for path in opts.args {
        let _ = io.write(format!("{}\n", path));
        let path = Path::new(&path);
        if let Ok(meta) = indexedlog::log::LogMetadata::read_file(path) {
            write!(io.output, "Metadata File {:?}\n{:?}\n", path, meta)?;
        } else if path.is_dir() {
            // Treate it as Log.
            let log = indexedlog::log::Log::open(path, Vec::new())?;
            write!(io.output, "Log Directory {:?}:\n{:#?}\n", path, log)?;
        } else if path.is_file() {
            // Treate it as Index.
            let idx = indexedlog::index::OpenOptions::new().open(path)?;
            write!(io.output, "Index File {:?}\n{:?}\n", path, idx)?;
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
