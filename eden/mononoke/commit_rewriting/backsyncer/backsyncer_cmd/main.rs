/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod cli;
mod run;
mod sharding;

use std::sync::Arc;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
use context::CoreContext;
use context::SessionContainer;
use cross_repo_sync::Repo as CrossRepo;
use executor_lib::RepoShardedProcessExecutor;
use fbinit::FacebookInit;
use metadata::Metadata;
use mononoke_app::MononokeApp;
use mononoke_app::MononokeAppBuilder;
use mononoke_app::monitoring::AliveService;
use mononoke_app::monitoring::MonitoringAppExtension;
use slog::info;

use crate::cli::BacksyncerArgs;
use crate::cli::SCUBA_TABLE;
use crate::sharding::APP_NAME;
use crate::sharding::BacksyncProcess;
use crate::sharding::BacksyncProcessExecutor;
use crate::sharding::SM_CLEANUP_TIMEOUT_SECS;

async fn async_main(ctx: CoreContext, app: MononokeApp) -> Result<(), Error> {
    let args: BacksyncerArgs = app.args()?;
    let ctx = Arc::new(ctx);
    let app = Arc::new(app);
    let repo_args = args.repo_args.clone();
    let runtime = app.runtime().clone();

    let res = if let Some(executor) = args.sharded_executor_args.clone().build_executor(
        app.fb,
        runtime.clone(),
        ctx.logger(),
        || Arc::new(BacksyncProcess::new(ctx.clone(), app.clone())),
        true, // enable shard (repo) level healing
        SM_CLEANUP_TIMEOUT_SECS,
    )? {
        let (sender, receiver) = tokio::sync::oneshot::channel::<bool>();
        executor.block_and_execute(ctx.logger(), receiver).await?;
        drop(sender);
        Ok(())
    } else {
        let repo_args = repo_args
            .into_source_and_target_args()
            .context("Source and Target repos must be provided when running in non-sharded mode")?;
        let process_executor = BacksyncProcessExecutor::new(ctx.clone(), app, repo_args).await?;
        process_executor.execute().await
    };

    if let Err(ref e) = res {
        let mut scuba = ctx.scuba().clone();
        scuba.log_with_msg("Execution error", e.to_string());
    }
    res
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app: MononokeApp = MononokeAppBuilder::new(fb)
        .with_app_extension(MonitoringAppExtension {})
        .with_default_scuba_dataset(SCUBA_TABLE)
        .build::<BacksyncerArgs>()?;
    let args: BacksyncerArgs = app.args()?;

    let mut metadata = Metadata::default();
    metadata.add_client_info(ClientInfo::default_with_entry_point(
        ClientEntryPoint::MegarepoBacksyncer,
    ));

    let scribe = args.scribe_logging_args.get_scribe(fb)?;

    let mut scuba = app.environment().scuba_sample_builder.clone();
    scuba.add_metadata(&metadata);
    scuba.add_common_server_data();

    let session_container = SessionContainer::builder(fb)
        .metadata(Arc::new(metadata))
        .build();

    let ctx = session_container.new_context_with_scribe(app.logger().clone(), scuba, scribe);

    info!(
        ctx.logger(),
        "Starting session with id {}",
        ctx.metadata().session_id(),
    );

    app.run_with_monitoring_and_logging(|app| async_main(ctx.clone(), app), APP_NAME, AliveService)
}
