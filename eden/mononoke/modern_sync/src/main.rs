/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(async_closure)]

use anyhow::Result;
use bookmarks::BookmarkUpdateLog;
use clap::Parser;
use commit_graph::CommitGraph;
use executor_lib::args::ShardedExecutorArgs;
use fbinit::FacebookInit;
use mononoke_app::args::RepoArgs;
use mononoke_app::args::RepoFilterAppExtension;
use mononoke_app::fb303::AliveService;
use mononoke_app::fb303::Fb303AppExtension;
use mononoke_app::MononokeApp;
use mononoke_app::MononokeAppBuilder;
use mutable_counters::MutableCounters;
use repo_blobstore::RepoBlobstore;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;

mod bul_util;
mod commands;
mod sender;
mod sync;

#[derive(Parser)]
struct ModernSyncArgs {
    #[clap(flatten)]
    sharded_executor_args: ShardedExecutorArgs,

    #[clap(flatten)]
    pub repo: RepoArgs,
}

#[facet::container]
#[derive(Clone)]
pub struct Repo {
    #[facet]
    pub mutable_counters: dyn MutableCounters,
    #[facet]
    repo_identity: RepoIdentity,
    #[facet]
    bookmark_update_log: dyn BookmarkUpdateLog,
    #[facet]
    commit_graph: CommitGraph,
    #[facet]
    repo_derived_data: RepoDerivedData,
    #[facet]
    repo_blobstore: RepoBlobstore,
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let subcommands = commands::subcommands();
    let app = MononokeAppBuilder::new(fb)
        .with_app_extension(Fb303AppExtension {})
        .with_app_extension(RepoFilterAppExtension {})
        .build_with_subcommands::<ModernSyncArgs>(subcommands)?;
    app.run_with_monitoring_and_logging(async_main, "modern_sync", AliveService)
}

async fn async_main(app: MononokeApp) -> Result<()> {
    commands::dispatch(app).await
}
