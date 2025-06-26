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

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use clap::Parser;
use edenfs_client::utils::get_mount_point;
use edenfs_client::utils::locate_repo_root;
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
    pub(crate) fn get_mount_point_and_search_root(&self) -> Result<(PathBuf, PathBuf)> {
        // Use absolute mount_point if provided (i.e. no search_root) else use
        // cwd as mount_point and compute search_root.
        let mount_point = get_mount_point(&self.mount_point)?;
        let mut search_root = PathBuf::new();

        // If mount_point is based on cwd - compute search_root
        if self.mount_point.is_none() {
            let cwd = std::env::current_dir()
                .with_context(|| "Unable to retrieve current working directory")?;
            search_root = cwd.strip_prefix(&mount_point)?.to_path_buf();
        } else {
            // validate absolute mount_point is point to root
            let repo_root =
                locate_repo_root(&mount_point).with_context(|| "Unable to locate repo root")?;
            if mount_point != repo_root {
                eprintln!(
                    "{} is not the root of an EdenFS repo",
                    mount_point.display()
                );
                return Err(anyhow!("Invalid mount point"));
            }
        }
        Ok((mount_point, search_root))
    }

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
