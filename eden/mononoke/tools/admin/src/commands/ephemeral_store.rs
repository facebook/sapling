/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod cleanup;
mod info;
mod list;

use anyhow::Result;
use clap::{Parser, Subcommand};
use cleanup::EphemeralStoreCleanUpArgs;
use info::EphemeralStoreInfoArgs;
use list::EphemeralStoreListArgs;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;

/// Cleanup, list or describe the contents of ephemeral store.
#[derive(Parser)]
pub struct CommandArgs {
    /// The repository name or ID.
    #[clap(flatten)]
    repo: RepoArgs,

    /// The subcommand for ephemeral store.
    #[clap(subcommand)]
    subcommand: EphemeralStoreSubcommand,
}

#[facet::container]
pub struct Repo {
    // TODO: Add necessary repo attributes here.
}

#[derive(Subcommand)]
pub enum EphemeralStoreSubcommand {
    /// Cleanup expired bubbles within the ephemeral store.
    Cleanup(EphemeralStoreCleanUpArgs),
    /// Describe metadata associated with a bubble within the ephemeral store.
    Info(EphemeralStoreInfoArgs),
    /// List out the keys of the blobs in a bubble within the ephemeral store.
    List(EphemeralStoreListArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_context();
    let logger = &app.logger();
    let repo: Repo = app.open_repo(&args.repo).await?;

    match args.subcommand {
        EphemeralStoreSubcommand::Cleanup(cleanup_args) => {
            cleanup::clean_bubbles(&ctx, &repo, logger, cleanup_args).await?
        }
        EphemeralStoreSubcommand::Info(info_args) => {
            info::bubble_info(&ctx, &repo, logger, info_args).await?
        }
        EphemeralStoreSubcommand::List(list_args) => {
            list::list_keys(&ctx, &repo, logger, list_args).await?
        }
    }
    Ok(())
}
