/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl notify changes-since

use std::path::PathBuf;

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use edenfs_client::types::JournalPosition;
use edenfs_client::EdenFsInstance;
use hg_util::path::expand_path;

use crate::ExitCode;

// TODO: add a --timeout flag
#[derive(Parser, Debug)]
#[clap(about = "Returns the changes since the given EdenFS journal position")]
pub struct ChangesSinceCmd {
    #[clap(long, short = 'p', allow_hyphen_values = true)]
    /// Journal position to start from
    position: JournalPosition,

    #[clap(parse(from_str = expand_path))]
    /// Path to the mount point
    mount_point: Option<PathBuf>,

    #[clap(long, help = "Include VCS roots in the output")]
    include_vcs_roots: bool,

    #[clap(
        long,
        help = "Included roots in the output. None means include all roots"
    )]
    included_roots: Option<Vec<PathBuf>>,

    #[clap(
        long,
        help = "Excluded roots in the output. None means exclude no roots"
    )]
    excluded_roots: Option<Vec<PathBuf>>,

    #[clap(
        long,
        help = "Included suffixes in the output. None means include all suffixes"
    )]
    included_suffixes: Option<Vec<String>>,

    #[clap(
        long,
        help = "Excluded suffixes in the output. None means exclude no suffixes"
    )]
    excluded_suffixes: Option<Vec<String>>,

    #[clap(long, help = "Print the output in JSON format")]
    json: bool,
}

#[async_trait]
impl crate::Subcommand for ChangesSinceCmd {
    #[cfg(not(fbcode_build))]
    async fn run(&self) -> Result<ExitCode> {
        eprintln!("not supported in non-fbcode build");
        Ok(1)
    }

    #[cfg(fbcode_build)]
    async fn run(&self) -> Result<ExitCode> {
        let instance = EdenFsInstance::global();
        let result = instance
            .get_changes_since(
                &self.mount_point,
                &self.position,
                self.include_vcs_roots,
                &self.included_roots,
                &self.excluded_roots,
                &self.included_suffixes,
                &self.excluded_suffixes,
                None,
            )
            .await?;
        println!(
            "{}",
            if self.json {
                serde_json::to_string(&result)?
            } else {
                result.to_string()
            }
        );
        Ok(0)
    }
}
