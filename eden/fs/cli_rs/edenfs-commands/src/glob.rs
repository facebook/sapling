/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl glob

use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use edenfs_client::utils::locate_repo_root;
use hg_util::path::expand_path;

use crate::ExitCode;
use crate::get_edenfs_instance;

#[derive(Parser, Debug)]
pub struct CommonArgs {
    #[clap(
        long,
        alias = "repo",
        help = "Specify path to mount point (default: root of cwd)",
        parse(from_str = expand_path)
    )]
    mount_point: Option<PathBuf>,

    #[clap(
        long,
        help = "Obtain patterns to match from FILE, one per line. If FILE is - , read patterns from standard input."
    )]
    pattern_file: Option<PathBuf>,

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
}

impl CommonArgs {
    fn load_patterns(&self) -> Result<Vec<String>> {
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

#[derive(Parser, Debug)]
#[clap(
    about = "Print matching filenames. Glob patterns can be provided via a pattern file. This command does not do any filtering based on source control state or gitignore files."
)]
pub struct GlobCmd {
    #[clap(flatten)]
    common: CommonArgs,

    #[clap(long, help = "Print the output in JSON format")]
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
        let _instance = get_edenfs_instance();

        // Get cwd mount_point if not provided.
        let current_dir: PathBuf;
        let mount_point = match &self.common.mount_point {
            Some(ref mount_point) => mount_point,
            None => {
                current_dir = std::env::current_dir()
                    .context("Unable to retrieve current working directory")?;
                &current_dir
            }
        };

        // Get mount_point as just repo root
        let repo_root = locate_repo_root(mount_point);

        // Get relative root (search root)
        let prefix = repo_root.unwrap_or_else(|| Path::new(""));
        let search_root = mount_point.strip_prefix(prefix);

        // Load patterns
        let patterns = self.common.load_patterns();

        // TEMP: debugging code
        println!(
            "mount_point = {:?}\nrepo_root = {:?}\nprefix = {:?}\nsearch_root = {:?}\npatterns = {:?}",
            mount_point, repo_root, prefix, search_root, patterns
        );
        Ok(0)
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
