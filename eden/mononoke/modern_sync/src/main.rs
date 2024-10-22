/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(async_closure)]

use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use executor_lib::args::ShardedExecutorArgs;
use fbinit::FacebookInit;
use metaconfig_types::ShardedService;
use mononoke_app::args::RepoFilterAppExtension;
use mononoke_app::fb303::AliveService;
use mononoke_app::fb303::Fb303AppExtension;
use mononoke_app::MononokeApp;
use mononoke_app::MononokeAppBuilder;
use mononoke_app::MononokeReposManager;
use mutable_counters::MutableCounters;
use slog::info;

mod commands;

#[derive(Parser)]
struct ModernSyncArgs {
    #[clap(flatten)]
    sharded_executor_args: ShardedExecutorArgs,
}

#[facet::container]
#[derive(Clone)]
pub struct Repo {
    #[facet]
    pub mutable_counters: dyn MutableCounters,
}

pub struct ModernSyncProcess {
    _fb: FacebookInit,
    _repos_mgr: Arc<MononokeReposManager<Repo>>,
}

impl ModernSyncProcess {
    fn new(fb: FacebookInit, repos_mgr: MononokeReposManager<Repo>) -> Self {
        let repos_mgr = Arc::new(repos_mgr);
        Self {
            _fb: fb,
            _repos_mgr: repos_mgr,
        }
    }
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
    let args: ModernSyncArgs = app.args()?;
    // Service name is used for shallow or deep sharding. If sharding itself is disabled, provide
    // service name as None while opening repos.
    let service_name = args
        .sharded_executor_args
        .sharded_service_name
        .as_ref()
        .map(|_| ShardedService::ModernSync);

    let repos_mgr = app.open_managed_repos_unredacted(service_name).await?;
    let repos = repos_mgr.repos().clone();

    let _process = Arc::new(ModernSyncProcess::new(app.fb, repos_mgr));

    info!(
        app.logger(),
        "ModernSync process initialized for repo {:?}",
        repos.iter_names().collect::<Vec<_>>()
    );
    commands::dispatch(app).await
}
