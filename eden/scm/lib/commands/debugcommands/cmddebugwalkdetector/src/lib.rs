/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;

use clidispatch::ReqCtx;
use cmdutil::Result;
use cmdutil::define_flags;
use types::RepoPathBuf;

define_flags! {
    pub struct DebugWalkDetectorOpts {
        /// Dir walk threshold
        dir_walk_threshold: Option<i64>,

        /// Read directory info and inject into walk detector. Assumes input paths are relative to CWD.
        inject_dir_hints: bool = false,

        /// Only test directory access. Implies --inject-dir-hints.
        dirs_only: bool = false,

        #[args]
        args: Vec<String>,
    }
}

pub fn run(ctx: ReqCtx<DebugWalkDetectorOpts>) -> Result<u8> {
    let detector = walkdetector::Detector::new();

    if let Some(threshold) = ctx.opts.dir_walk_threshold {
        detector.set_min_dir_walk_threshold(threshold as usize);
    }

    let mut seen_dirs = HashSet::new();
    let cwd = std::env::current_dir()?;

    let input = ctx.io().input();
    let input = BufReader::new(input);
    for line in input.lines() {
        let file_path: RepoPathBuf = line?.try_into()?;

        if ctx.opts.inject_dir_hints || ctx.opts.dirs_only {
            for parent in file_path.parents() {
                let dir = cwd.join(parent.to_path());
                if !seen_dirs.insert(dir.clone()) {
                    continue;
                }

                let mut num_files = 0;
                let mut num_dirs = 0;
                for entry in std::fs::read_dir(&dir)? {
                    let file_type = entry?.file_type()?;
                    if file_type.is_dir() {
                        num_dirs += 1;
                    } else if file_type.is_file() || file_type.is_symlink() {
                        num_files += 1;
                    }
                }
                detector.dir_read(parent.to_owned(), num_files, num_dirs);
            }
        }

        if !ctx.opts.dirs_only {
            detector.file_read(file_path);
        }
    }

    let mut output = ctx.io().output();

    writeln!(output, "File walks:")?;
    for (root, depth) in detector.file_walks() {
        writeln!(output, "root: {root}, depth: {depth}")?;
    }

    writeln!(output, "\nDir walks:")?;
    for (root, depth) in detector.dir_walks() {
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
