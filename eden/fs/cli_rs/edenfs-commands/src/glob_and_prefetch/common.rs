/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use hg_util::path::expand_path;

#[derive(Parser, Debug)]
pub(crate) struct CommonArgs {
    #[clap(
        long,
        alias = "repo",
        help = "Specify path to mount point (default: root of cwd)",
        parse(from_str = expand_path)
    )]
    pub(crate) mount_point: Option<PathBuf>,

    #[clap(
        long,
        help = "Obtain patterns to match from FILE, one per line. If FILE is - , read patterns from standard input."
    )]
    pub(crate) pattern_file: Option<PathBuf>,

    // Technically, we use fnmatch, but it uses glob for pattern strings.
    // source: https://man7.org/linux/man-pages/man3/fnmatch.3.html
    #[clap(
        help = "Filename patterns (relative to repo root) to match via glob, see: https://man7.org/linux/man-pages/man7/glob.7.html"
    )]
    pub(crate) pattern: Vec<String>,

    #[clap(
        long,
        help = "When printing the list of matching files, exclude directories."
    )]
    pub(crate) list_only_files: bool,

    #[clap(
        long,
        help = "When printing the list of matching files, exclude directories."
    )]
    pub(crate) include_dot_files: bool,
}

impl CommonArgs {
    pub(crate) fn load_patterns(&self) -> Result<Vec<String>> {
        let mut pattern = self
            .pattern
            .iter()
            .map(|p| clean_pattern(p.to_string()))
            .collect::<Vec<String>>();
        match &self.pattern_file {
            Some(pattern_file) => {
                let file = File::open(pattern_file)?;
                let reader = BufReader::new(file);
                for line in reader.lines() {
                    pattern.push(clean_pattern(line?));
                }

                Ok(pattern)
            }
            None => Ok(pattern),
        }
    }
}

#[cfg(target_os = "windows")]
fn clean_pattern(pattern: String) -> String {
    pattern.replace("\\", "/")
}

#[cfg(not(target_os = "windows"))]
fn clean_pattern(pattern: String) -> String {
    pattern
}
