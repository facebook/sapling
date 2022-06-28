/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod inspect;
mod last_processed;
mod remains;
mod show;
mod verify;

use anyhow::Context;
use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
use bookmarks::BookmarkUpdateLog;
use clap::Parser;
use clap::Subcommand;
use ephemeral_blobstore::RepoEphemeralStore;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use mutable_counters::MutableCounters;
use repo_blobstore::RepoBlobstore;
use repo_identity::RepoIdentity;

use inspect::HgSyncInspectArgs;
use last_processed::HgSyncLastProcessedArgs;
use remains::HgSyncRemainsArgs;
use show::HgSyncShowArgs;
use verify::HgSyncVerifyArgs;

const LATEST_REPLAYED_REQUEST_KEY: &str = "latest-replayed-request";

/// Inspect and interact with synchronization to Mercurial repositories.
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    #[clap(subcommand)]
    subcommand: HgSyncSubcommand,
}

#[facet::container]
pub struct Repo {
    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    bonsai_globalrev_mapping: dyn BonsaiGlobalrevMapping,

    #[facet]
    bonsai_svnrev_mapping: dyn BonsaiSvnrevMapping,

    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    repo_ephemeral_store: RepoEphemeralStore,

    #[facet]
    bookmark_update_log: dyn BookmarkUpdateLog,

    #[facet]
    mutable_counters: dyn MutableCounters,
}

#[derive(Subcommand)]
pub enum HgSyncSubcommand {
    /// Print information about a hg-sync log entry
    Inspect(HgSyncInspectArgs),
    /// Show or update the last processed log entry id
    LastProcessed(HgSyncLastProcessedArgs),
    /// Show how many bundles are remaining to be synced
    Remains(HgSyncRemainsArgs),
    /// Show the next bundles to be synced
    Show(HgSyncShowArgs),
    /// Verify that all remaining bundles are of consistent types
    Verify(HgSyncVerifyArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_context();

    let repo: Repo = app
        .open_repo(&args.repo)
        .await
        .context("Failed to open repo")?;

    match args.subcommand {
        HgSyncSubcommand::Inspect(inspect_args) => {
            inspect::inspect(&ctx, &repo, inspect_args).await?
        }
        HgSyncSubcommand::LastProcessed(last_processed_args) => {
            last_processed::last_processed(&ctx, &repo, last_processed_args).await?
        }
        HgSyncSubcommand::Remains(remains_args) => {
            remains::remains(&ctx, &repo, remains_args).await?
        }
        HgSyncSubcommand::Show(show_args) => show::show(&ctx, &repo, show_args).await?,
        HgSyncSubcommand::Verify(verify_args) => verify::verify(&ctx, &repo, verify_args).await?,
    }

    Ok(())
}
