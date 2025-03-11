/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(async_closure)]

use std::path::PathBuf;

use anyhow::Result;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::BookmarkUpdateLog;
use bookmarks::Bookmarks;
use clap::Parser;
use commit_graph::CommitGraph;
use executor_lib::args::ShardedExecutorArgs;
use fbinit::FacebookInit;
use metaconfig_types::RepoConfig;
use mononoke_app::args::RepoArgs;
use mononoke_app::args::RepoFilterAppExtension;
use mononoke_app::args::TLSArgs;
use mononoke_app::monitoring::AliveService;
use mononoke_app::monitoring::MonitoringAppExtension;
use mononoke_app::MononokeApp;
use mononoke_app::MononokeAppBuilder;
use mutable_counters::MutableCounters;
use repo_blobstore::RepoBlobstore;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;

mod bul_util;
mod commands;
mod scuba;
mod sender;
mod sync;

#[derive(Parser)]
struct ModernSyncArgs {
    #[clap(flatten)]
    sharded_executor_args: ShardedExecutorArgs,

    #[clap(flatten)]
    pub repo: RepoArgs,

    #[clap(long, hide = true)] // To be used only in integration tests
    pub dest_socket: Option<u64>,

    /// TLS parameters for this service
    #[clap(flatten)]
    tls_params: Option<TLSArgs>,

    #[clap(long)]
    /// "Dest repo name (in case it's different from source repo name)"
    dest_repo_name: Option<String>,

    #[clap(long)]
    /// Update counters in the source repo (prod and tests only)
    update_counters: bool,

    #[clap(long)]
    /// Update ODS counters
    log_to_ods: bool,

    #[clap(long)]
    /// Path to file to check for in order to exit
    exit_file: PathBuf,
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
    #[facet]
    pub repo_config: RepoConfig,
    #[facet]
    bookmarks: dyn Bookmarks,
    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let subcommands = commands::subcommands();
    let app = MononokeAppBuilder::new(fb)
        .with_default_scuba_dataset("mononoke_modern_sync")
        .with_app_extension(MonitoringAppExtension {})
        .with_app_extension(RepoFilterAppExtension {})
        .build_with_subcommands::<ModernSyncArgs>(subcommands)?;
    app.run_with_monitoring_and_logging(async_main, "modern_sync", AliveService)
}

async fn async_main(app: MononokeApp) -> Result<()> {
    commands::dispatch(app).await
}
