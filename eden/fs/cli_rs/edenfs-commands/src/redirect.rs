/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl redirect

use anyhow::anyhow;
use anyhow::Context;
use async_trait::async_trait;
use clap::Parser;
use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use tabular::row;
use tabular::Table;

use edenfs_client::checkout::find_checkout;
use edenfs_client::redirect::get_effective_redirections;
use edenfs_client::redirect::Redirection;
use edenfs_client::redirect::RedirectionState;
use edenfs_client::EdenFsInstance;
use edenfs_error::Result;

use crate::util::expand_path_or_cwd;
use crate::ExitCode;
use crate::Subcommand;

#[derive(Parser, Debug)]
#[clap(name = "redirect")]
#[clap(about = "List and manipulate redirected paths")]
pub enum RedirectCmd {
    List {
        #[clap(
            long,
            parse(try_from_str = expand_path_or_cwd),
            default_value = "",
            help = "The EdenFS mount point path."
        )]
        mount: PathBuf,
        #[clap(long, help = "output in json rather than human readable text")]
        json: bool,
    },
}

impl RedirectCmd {
    fn print_redirection_table(
        &self,
        redirections: BTreeMap<PathBuf, Redirection>,
    ) -> Result<ExitCode> {
        let mut table = Table::new("{:<}    {:<}    {:<}    {:<}    {:<}");
        table.add_row(row!("REPO_PATH", "TYPE", "TARGET", "SOURCE", "STATE"));
        for redir in redirections.into_values() {
            table.add_row(row!(
                redir.repo_path().display(),
                redir.redir_type,
                redir
                    .target
                    .map(|x| x.display().to_string())
                    .unwrap_or_default(),
                redir.source,
                redir.state.unwrap_or(RedirectionState::UnknownMount),
            ));
        }
        println!("{}", table);
        Ok(0)
    }

    fn print_redirection_json(
        &self,
        redirections: BTreeMap<PathBuf, Redirection>,
    ) -> Result<ExitCode> {
        let json_out = serde_json::to_string(&redirections.into_values().collect::<Vec<_>>())
            .with_context(|| anyhow!("could not serialize redirections",))?;
        println!("{}", json_out);
        Ok(0)
    }

    async fn list(&self, instance: EdenFsInstance, mount: &Path, json: bool) -> Result<ExitCode> {
        let checkout = find_checkout(&instance, mount)?;
        let mut redirections = get_effective_redirections(&checkout).with_context(|| {
            anyhow!(
                "Unable to retrieve redirections for checkout '{}'",
                mount.display()
            )
        })?;

        redirections
            .values_mut()
            .map(|v| v.update_target_abspath(&checkout))
            .collect::<Result<Vec<()>>>()
            .with_context(|| anyhow!("failed to expand redirection target path"))?;

        if json {
            self.print_redirection_json(redirections)
        } else {
            self.print_redirection_table(redirections)
        }
    }
}

#[async_trait]
impl Subcommand for RedirectCmd {
    async fn run(&self, instance: EdenFsInstance) -> Result<ExitCode> {
        match self {
            Self::List { mount, json } => self.list(instance, mount, *json).await,
        }
    }
}
