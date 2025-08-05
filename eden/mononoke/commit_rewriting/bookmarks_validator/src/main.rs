/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod run;
mod sharding;

use std::sync::Arc;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use clap::Parser;
use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
use context::CoreContext;
use context::SessionContainer;
use executor_lib::RepoShardedProcessExecutor;
use executor_lib::args::ShardedExecutorArgs;
use fbinit::FacebookInit;
use metadata::Metadata;
use mononoke_app::MononokeApp;
use mononoke_app::MononokeAppBuilder;
use mononoke_app::args::OptSourceAndTargetRepoArgs;
use mononoke_app::monitoring::AliveService;
use mononoke_app::monitoring::MonitoringAppExtension;
use slog::info;

use crate::sharding::BookmarkValidateProcess;
use crate::sharding::BookmarkValidateProcessExecutor;

const SM_CLEANUP_TIMEOUT_SECS: u64 = 120;

#[derive(Debug, Parser)]
#[clap(about = "Tool to validate that small and large repo bookmarks are in sync")]
pub struct BookmarkValidatorArgs {
    #[clap(flatten)]
    pub sharded_executor_args: ShardedExecutorArgs,
    #[clap(flatten, next_help_heading = "CROSS REPO OPTIONS")]
    pub repo_args: OptSourceAndTargetRepoArgs,
}

async fn async_main(app: MononokeApp, ctx: CoreContext) -> Result<(), Error> {
    let args: BookmarkValidatorArgs = app.args()?;
    let ctx = Arc::new(ctx);
    let app = Arc::new(app);
    let repo_args = args.repo_args.clone();
    let runtime = app.runtime().clone();

    if let Some(executor) = args.sharded_executor_args.clone().build_executor(
        app.fb,
        runtime.clone(),
        ctx.logger(),
        || Arc::new(BookmarkValidateProcess::new(ctx.clone(), app.clone())),
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
        let x_repo_process_executor =
            BookmarkValidateProcessExecutor::new(ctx.clone(), app, repo_args).await?;
        x_repo_process_executor.execute().await
    }
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let app: MononokeApp = MononokeAppBuilder::new(fb)
        .with_app_extension(MonitoringAppExtension {})
        .build::<BookmarkValidatorArgs>()?;

    let mut metadata = Metadata::default();
    metadata.add_client_info(ClientInfo::default_with_entry_point(
        ClientEntryPoint::MegarepoBookmarksValidator,
    ));

    let mut scuba = app.environment().scuba_sample_builder.clone();
    scuba.add_metadata(&metadata);

    let session_container = SessionContainer::builder(fb)
        .metadata(Arc::new(metadata))
        .build();

    let ctx = session_container.new_context(app.logger().clone(), scuba);

    info!(
        ctx.logger(),
        "Starting session with id {}",
        ctx.metadata().session_id(),
    );

    app.run_with_monitoring_and_logging(
        |app| async_main(app, ctx.clone()),
        "bookmarks_validator",
        AliveService,
    )
}
