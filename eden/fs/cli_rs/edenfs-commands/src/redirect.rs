/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl redirect

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use edenfs_client::checkout::find_checkout;
use edenfs_client::redirect::get_effective_redirections;
use edenfs_client::redirect::Redirection;
use edenfs_client::redirect::RedirectionState;
use edenfs_client::EdenFsInstance;
use hg_util::path::expand_path;
use tabular::row;
use tabular::Table;

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
    Add {
        #[clap(long, parse(try_from_str = expand_path_or_cwd), default_value = "", help = "The EdenFS mount point path.")]
        mount: PathBuf,
        #[clap(parse(from_str = expand_path), index = 1, help = "The path in the repo which should be redirected")]
        repo_path: PathBuf,
        #[clap(index = 2, help = "The type of the redirection", possible_values = ["bind", "symlink"])]
        redir_type: String,
        #[clap(
            long,
            help = "Unmount and re-bind mount any bind mount redirections to \
            ensure that they are pointing to the right place. This is not the \
            default behavior in the interest of preserving kernel caches."
        )]
        force_remount_bind_mounts: bool,
        #[clap(
            long,
            help = "force the bind mount to fail if it would overwrite a \
            pre-existing directory"
        )]
        strict: bool,
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

    async fn list(&self, mount: &Path, json: bool) -> Result<ExitCode> {
        let instance = EdenFsInstance::global();
        let checkout = find_checkout(instance, mount)?;
        let mut redirections = get_effective_redirections(&checkout).with_context(|| {
            anyhow!(
                "Unable to retrieve redirections for checkout '{}'",
                mount.display()
            )
        })?;

        redirections
            .values_mut()
            .map(|v| v.update_target_abspath(&checkout))
            .collect::<Result<Vec<()>, _>>()
            .with_context(|| anyhow!("failed to expand redirection target path"))?;

        if json {
            self.print_redirection_json(redirections)
        } else {
            self.print_redirection_table(redirections)
        }
    }

    async fn add(
        &self,
        _mount: &Path,
        _repo_path: &Path,
        _redir_type: &str,
        _force_remount_bind_mounts: bool,
        _strict: bool,
    ) -> Result<ExitCode> {
        eprintln!("Using Rust version of `eden redirect add` which is unimplemented.");
        Ok(0)
    }
}

#[async_trait]
impl Subcommand for RedirectCmd {
    async fn run(&self) -> Result<ExitCode> {
        match self {
            Self::List { mount, json } => self.list(mount, *json).await,
            Self::Add {
                mount,
                repo_path,
                redir_type,
                force_remount_bind_mounts,
                strict,
            } => {
                self.add(
                    mount,
                    repo_path,
                    redir_type,
                    *force_remount_bind_mounts,
                    *strict,
                )
                .await
            }
        }
    }
}
