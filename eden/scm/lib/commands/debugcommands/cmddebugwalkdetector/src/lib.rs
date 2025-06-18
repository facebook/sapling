/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use clidispatch::ReqCtx;
use cmdutil::Result;
use cmdutil::define_flags;
use types::RepoPathBuf;

define_flags! {
    pub struct DebugWalkDetectorOpts {
        /// Walk threshold
        walk_threshold: Option<i64>,

        /// Walk ratio
        walk_ratio: Option<String>,

        /// Lax depth
        lax_depth: Option<i64>,

        /// Strict multiplier
        strict_multiplier: Option<i64>,

        /// Read directory info and inject into walk detector. Assumes input paths are relative to CWD.
        inject_dir_hints: bool = false,

        /// Only test directory access. Implies --inject-dir-hints.
        dirs_only: bool = false,

        /// Submit file accesses to walk detector from multiple threads.
        threads: i64 = 1,

        /// File to read input file paths from. Defaults to stdin.
        input_file: Option<String>,

        /// Wait for GC to collect everything at the end.
        wait_for_gc: bool = true,

        #[args]
        args: Vec<String>,
    }
}

#[derive(Clone)]
enum Work {
    File(RepoPathBuf),
    Dir(RepoPathBuf, usize, usize),
}

pub fn run(ctx: ReqCtx<DebugWalkDetectorOpts>) -> Result<u8> {
    let mut detector = walkdetector::Detector::new();

    if let Some(threshold) = ctx.opts.walk_threshold {
        detector.set_walk_threshold(threshold as usize);
    }

    if let Some(ratio) = &ctx.opts.walk_ratio {
        detector.set_walk_ratio(ratio.parse()?);
    }

    if let Some(lax_depth) = ctx.opts.lax_depth {
        detector.set_lax_depth(lax_depth as usize);
    }

    if let Some(multiplier) = ctx.opts.strict_multiplier {
        detector.set_strict_multiplier(multiplier as usize);
    }

    let detector = Arc::new(detector);

    let mut seen_dirs = HashSet::new();
    let cwd = std::env::current_dir()?;

    let input: Box<dyn Read> = if let Some(path) = &ctx.opts.input_file {
        Box::new(File::open(path)?)
    } else {
        Box::new(ctx.io().input())
    };

    let input = BufReader::new(input);
    let mut work = Vec::new();
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

                work.push(Work::Dir(parent.to_owned(), num_files, num_dirs));
            }
        }

        if !ctx.opts.dirs_only {
            work.push(Work::File(file_path));
        }
    }

    let chunk_size = work.len() / ctx.opts.threads as usize;
    let mut handles = Vec::new();
    for chunk in work.chunks(chunk_size) {
        let detector = detector.clone();
        let chunk = chunk.to_vec();
        handles.push(std::thread::spawn(move || {
            for work in chunk {
                match work {
                    Work::File(file_path) => {
                        detector.file_loaded(file_path, 0);
                    }
                    Work::Dir(dir, num_files, num_dirs) => {
                        detector.dir_loaded(dir, num_files, num_dirs, 0);
                    }
                }
            }
        }));
    }

    for handle in handles {
        handle.join().unwrap();
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

    if ctx.opts.wait_for_gc {
        while !detector.file_walks().is_empty() || !detector.dir_walks().is_empty() {
            std::thread::sleep(Duration::from_secs(1));
        }
        writeln!(output, "\nGC done!")?;
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

pub fn enable_cas() -> bool {
    false
}
