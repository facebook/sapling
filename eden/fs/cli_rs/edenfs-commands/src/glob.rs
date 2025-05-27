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
use edenfs_client::client;
use edenfs_client::glob_files::Glob;
use edenfs_client::glob_files::dtype_to_str;
use edenfs_client::utils::locate_repo_root;
use edenfs_utils::path_from_bytes;
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

impl GlobCmd {
    fn print_result(&self, result: &Glob) -> Result<()> {
        if self.json {
            println!(
                "{}\n",
                serde_json::to_string(&result)
                    .with_context(|| "Failed to serialize result to JSON.")?
            );
        } else {
            if result.matching_files.len() != result.origin_hashes.len()
                || (self.dtype && result.matching_files.len() != result.dtypes.len())
            {
                println!("Error globbing files: mismatched results")
            }

            for i in 0..result.matching_files.len() {
                print!(
                    "{:?}",
                    path_from_bytes(result.matching_files[i].as_ref())?
                        .to_string_lossy()
                        .to_string()
                );
                if self.list_origin_hash {
                    print!("@{}", hex::encode(&result.origin_hashes[i]));
                }
                if self.dtype {
                    print!(" {}", dtype_to_str(&result.dtypes[i]));
                }
                println!();
            }

            if self.verbose {
                println!("Num matching files: {}", result.matching_files.len());
                println!("Num dtypes: {}", result.dtypes.len());
                println!("Num origin hashes: {}", result.origin_hashes.len());
            }
        }
        Ok(())
    }
}

#[async_trait]
impl crate::Subcommand for GlobCmd {
    async fn run(&self) -> Result<ExitCode> {
        let instance = get_edenfs_instance();
        let client = instance.get_client();

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
        let repo_root = repo_root.unwrap_or_else(|| Path::new(""));
        let search_root = mount_point.strip_prefix(repo_root)?;

        // Load patterns
        let patterns = self.common.load_patterns()?;

        let glob = client
            .glob_files(
                repo_root,
                patterns,
                self.common.include_dot_files,
                false,
                false,
                self.dtype,
                search_root,
                false,
                self.common.list_only_files,
            )
            .await?;

        self.print_result(&glob)?;

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
