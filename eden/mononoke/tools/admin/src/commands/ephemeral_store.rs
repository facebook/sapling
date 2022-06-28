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
use clap::Parser;
use clap::Subcommand;
use cleanup::EphemeralStoreCleanUpArgs;
use ephemeral_blobstore::EphemeralBlobstoreError;
use ephemeral_blobstore::RepoEphemeralStore;
use info::EphemeralStoreInfoArgs;
use list::EphemeralStoreListArgs;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use repo_identity::RepoIdentity;

/// Cleanup, list or describe the contents of ephemeral store.
#[derive(Parser)]
pub struct CommandArgs {
    /// The repository name or ID. Any changesets provided for
    /// subcommands will use this repoID for scoping.
    #[clap(flatten)]
    repo: RepoArgs,

    /// The subcommand for ephemeral store.
    #[clap(subcommand)]
    subcommand: EphemeralStoreSubcommand,
}

#[facet::container]
pub struct Repo {
    #[facet]
    repo_ephemeral_store: RepoEphemeralStore,
    #[facet]
    repo_identity: RepoIdentity,
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
    let repo: Repo = app.open_repo(&args.repo).await?;

    let subcommand_result = match args.subcommand {
        EphemeralStoreSubcommand::Cleanup(cleanup_args) => {
            cleanup::clean_bubbles(&ctx, &repo, cleanup_args).await
        }
        EphemeralStoreSubcommand::Info(info_args) => info::bubble_info(&repo, info_args).await,
        EphemeralStoreSubcommand::List(list_args) => list::list_keys(&ctx, &repo, list_args).await,
    };
    match subcommand_result {
        Err(e) => match e.downcast_ref::<EphemeralBlobstoreError>() {
            // If this repo does not have an ephemeral store, then this is a no-op
            Some(EphemeralBlobstoreError::NoEphemeralBlobstore(repo_id)) => {
                println!(
                    "Ephemeral store doesn't exist for repo {}, exiting.",
                    repo_id
                );
                Ok(())
            }
            // For any other error, return as-is
            _ => Err(e),
        },
        _ => Ok(()),
    }
}
