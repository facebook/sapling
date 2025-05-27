/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl glob

use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use edenfs_client::glob_files::Glob;
use edenfs_client::glob_files::dtype_to_str;
use edenfs_client::utils::get_mount_point;
use edenfs_client::utils::locate_repo_root;
use edenfs_utils::path_from_bytes;

use crate::ExitCode;
use crate::get_edenfs_instance;
use crate::glob_and_prefetch::common::CommonArgs;

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

        // Use absolute mount_point if provided (i.e. no search_root) else use
        // cwd as mount_point and compute search_root.
        let mount_point = get_mount_point(&self.common.mount_point)?;
        let mut search_root = PathBuf::new();

        // If mount_point is based on cwd - compute search_root
        if self.common.mount_point.is_none() {
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
                return Ok(1);
            }
        }

        // Load patterns
        let patterns = self.common.load_patterns()?;

        let glob = client
            .glob_files(
                &mount_point,
                patterns,
                self.common.include_dot_files,
                false,
                false,
                self.dtype,
                &search_root,
                false,
                self.common.list_only_files,
            )
            .await?;

        self.print_result(&glob)?;

        Ok(0)
    }
}
