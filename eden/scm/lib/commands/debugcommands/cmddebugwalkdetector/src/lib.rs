/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::time::Instant;

use clidispatch::ReqCtx;
use cmdutil::Result;
use cmdutil::define_flags;

define_flags! {
    pub struct DebugWalkDetectorOpts {
        /// Dir walk threshold
        dir_walk_threshold: Option<i64>,

        #[args]
        args: Vec<String>,
    }
}

pub fn run(ctx: ReqCtx<DebugWalkDetectorOpts>) -> Result<u8> {
    let detector = walkdetector::Detector::new();

    if let Some(threshold) = ctx.opts.dir_walk_threshold {
        detector.set_min_dir_walk_threshold(threshold as usize);
    }

    let input = ctx.io().input();
    let input = BufReader::new(input);
    for line in input.lines() {
        detector.file_read(Instant::now(), line.unwrap().try_into().unwrap());
    }

    let mut output = ctx.io().output();
    writeln!(output, "Final walks:")?;

    for (root, depth) in detector.walks() {
        writeln!(output, "root: {root}, depth: {depth}")?;
    }

    Ok(0)
}

pub fn aliases() -> &'static str {
    "debugwalkdetector"
}

pub fn doc() -> &'static str {
    "debug filesystem walk detector"
}

pub fn synopsis() -> Option<&'static str> {
    None
}
