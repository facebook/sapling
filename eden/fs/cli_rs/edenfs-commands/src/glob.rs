/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl glob

use std::path::PathBuf;

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;

use crate::ExitCode;
use crate::get_edenfs_instance;

#[derive(Parser, Debug)]
#[clap(
    about = "Print matching filenames",
    long_about = "Print matching filenames. Glob patterns can be provided via a pattern file. This command does not do any filtering based on source control state or gitignore files."
)]
pub struct GlobCmd {
    #[clap(long, help = "Specify path to repo root (default: root of cwd)")]
    repo: PathBuf,

    #[clap(
        long,
        help = "Obtain patterns to match from FILE, one per line. If FILE is - , read patterns from standard input."
    )]
    pattern_file: PathBuf,

    // Technically, we use fnmatch, but it uses glob for pattern strings.
    // source: https://man7.org/linux/man-pages/man3/fnmatch.3.html
    #[clap(
        help = "Filename patterns (relative to repo root) to match via glob, see: https://man7.org/linux/man-pages/man7/glob.7.html"
    )]
    pattern: Vec<String>,

    #[clap(
        long,
        help = "When printing the list of matching files, exclude directories."
    )]
    list_only_files: bool,

    #[clap(
        long,
        help = "When printing the list of matching files, exclude directories."
    )]
    include_dot_files: bool,

    #[clap(long, help = "Return results as JSON")]
    json: bool,

    #[clap(long, help = "Display additional data")]
    verbose: bool,

    #[clap(long, help = "Display the origin hash of the matching files.")]
    list_origin_hash: bool,

    #[clap(long, help = "Display the dtype of the matching files.")]
    dtype: bool,

    #[clap(long, help = "Revisions to search within. Can be used multiple times")]
    revision: Option<Vec<String>>,
}

#[async_trait]
impl crate::Subcommand for GlobCmd {
    async fn run(&self) -> Result<ExitCode> {
        let instance = get_edenfs_instance();
        Ok(0)
    }
}
