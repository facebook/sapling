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
use edenfs_client::glob_files::Glob;
use edenfs_utils::path_from_bytes;

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
        let client = instance.get_client();
        let (mount_point, _search_root) = self.common.get_mount_point_and_seach_root()?;
        let patterns = self.common.load_patterns()?;

        let silent = self.silent || !self.debug_print;
        let return_prefetched_files = !(self.background || silent);
        let result = client
            .prefetch_files(
                &mount_point,
                patterns.clone(),
                self.directories_only,
                None,
                None::<PathBuf>,
                Some(self.background),
                None,
                return_prefetched_files,
            )
            .await?;

        if return_prefetched_files {
            if !patterns.is_empty()
                && result
                    .prefetched_files
                    .as_ref()
                    .is_none_or(|pf| pf.matching_files.is_empty())
            {
                eprint!("No files were matched by the pattern");
                if !patterns.is_empty() {
                    eprint!("s");
                }
                eprintln!(" specified.\nSee `eden prefetch -h` for docs on pattern matching.");
            }

            if let Some(prefetched_files) = &result.prefetched_files {
                if self.debug_print {
                    for file in &prefetched_files.matching_files {
                        println!("{}", path_from_bytes(file)?.display());
                    }
                }
            }
        }

        Ok(0)
    }
}
