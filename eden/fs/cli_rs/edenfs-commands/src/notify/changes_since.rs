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
use edenfs_client::changes_since::ChangesSinceV2Result;
use edenfs_client::journal_position::JournalPosition;
use edenfs_client::EdenFsInstance;
use edenfs_error::EdenFsError;
use edenfs_error::ResultExt;
use hg_util::path::expand_path;

use crate::ExitCode;

// TODO: add a --timeout flag
#[derive(Parser, Debug)]
#[clap(about = "Returns the changes since the given EdenFS journal position")]
pub struct ChangesSinceCmd {
    #[clap(
        long,
        short = 'p',
        allow_hyphen_values = true,
        required_unless_present_any = ["subscribe"],
    )]
    /// Journal position to start from
    position: Option<JournalPosition>,

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

    #[clap(
        long,
        alias = "wait",
        help = "Return any immediate changes as well as following instances of changes"
    )]
    subscribe: bool,

    #[clap(long, help = "Print the output in JSON format")]
    json: bool,

    #[clap(short, long, default_value = "0")]
    /// [Unit: ms] number of milliseconds to wait between events
    throttle: u64,
}

impl ChangesSinceCmd {
    fn print_result(&self, result: &ChangesSinceV2Result) -> Result<(), EdenFsError> {
        println!(
            "{}",
            if self.json {
                serde_json::to_string(&result).from_err()? + "\n"
            } else {
                result.to_string()
            }
        );

        Ok(())
    }
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
        let client = instance.get_client(None).await?;
        let position = self
            .position
            .clone()
            .unwrap_or(client.get_journal_position(&self.mount_point).await?);
        let result = client
            .get_changes_since(
                &self.mount_point,
                &position,
                &None,
                &self.included_roots,
                &self.included_suffixes,
                &self.excluded_roots,
                &self.excluded_suffixes,
                self.include_vcs_roots,
                None,
            )
            .await?;

        self.print_result(&result)?;
        if self.subscribe {
            instance
                .subscribe(
                    &self.mount_point,
                    self.throttle,
                    Some(position),
                    &None,
                    &self.included_roots,
                    &self.included_suffixes,
                    &self.excluded_roots,
                    &self.excluded_suffixes,
                    self.include_vcs_roots,
                    |result| self.print_result(result),
                )
                .await?;
        }
        Ok(0)
    }
}
