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
use edenfs_client::utils::locate_repo_root;

use crate::ExitCode;
use crate::get_edenfs_instance;
use crate::glob_and_prefetch::common::CommonArgs;

#[derive(Parser, Debug)]
#[clap(
    about = "Prefetch content for matching file patterns. Glob patterns can be provided via a pattern file. This command does not do any filtering based on source control state or gitignore files."
)]
pub struct PrefetchCmd {
    #[clap(flatten)]
    common: CommonArgs,

    #[clap(
        long,
        help = "DEPRECATED: Do not print the names of the matching files"
    )]
    silent: bool,

    #[clap(long, help = "Do not prefetch files; only prefetch directories")]
    directories_only: bool,

    #[clap(long, help = "Run the prefetch in the background")]
    background: bool,

    #[clap(
        long,
        help = "Print the paths being prefetched. Does not work if using --background"
    )]
    debug_print: bool,
}

impl PrefetchCmd {
    fn _print_result(&self, _result: &Glob) -> Result<()> {
        Ok(())
    }
}

#[async_trait]
impl crate::Subcommand for PrefetchCmd {
    async fn run(&self) -> Result<ExitCode> {
        // TODO: add in telemetry support
        let instance = get_edenfs_instance();
        let _client = instance.get_client();

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
        let _repo_root = locate_repo_root(mount_point);

        // Load patterns
        let _patterns = self.common.load_patterns()?;

        // TODO: invoke prefetch or glob based on params and fallback

        Ok(0)
    }
}
