/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub(crate) mod abort;
mod fail_dead_ready;
mod list;
mod requeue;
mod show;
mod show_megarepo_sync_target_config;
mod submit;

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use context::SessionContainer;
use metaconfig_types::RepoConfigArc;
use mononoke_api::Repo;
use mononoke_app::MononokeApp;
use submit::AsyncRequestsSubmitArgs;

use crate::commands::async_requests::abort::AsyncRequestsAbortArgs;
use crate::commands::async_requests::fail_dead_ready::AsyncRequestsFailDeadReadyRequestsArgs;
use crate::commands::async_requests::list::AsyncRequestsListArgs;
use crate::commands::async_requests::requeue::AsyncRequestsRequeueArgs;
use crate::commands::async_requests::show::AsyncRequestsShowArgs;
use crate::commands::async_requests::show_megarepo_sync_target_config::AsyncRequestsShowMegarepoSyncTargetConfigArgs;

/// View and manage the SCS async requests
#[derive(Parser)]
pub struct CommandArgs {
    /// The subcommand for async requests
    #[clap(subcommand)]
    subcommand: AsyncRequestsSubcommand,
}

#[derive(Subcommand)]
pub enum AsyncRequestsSubcommand {
    /// Lists asynchronous requests (by default the ones active
    /// now or updated within last 5 mins).
    List(AsyncRequestsListArgs),
    /// Marks "dead" ready requests (whose params blob is missing from the
    /// blobstore, i.e. `show` fails with "Missing blob") as failed.
    FailDeadReadyRequests(AsyncRequestsFailDeadReadyRequestsArgs),
    /// Shows request details.
    Show(AsyncRequestsShowArgs),
    /// Changes the request status to new so it's picked up
    /// by workers again.
    Requeue(AsyncRequestsRequeueArgs),
    /// Changes the request status to ready and put error as result.
    /// (this won't stop any currently running workers immediately)
    Abort(AsyncRequestsAbortArgs),
    /// Submits an async request. Intended only for development and testing.
    Submit(AsyncRequestsSubmitArgs),
    /// Shows the contents of a megarepo SyncTargetConfig by version string.
    ShowMegarepoSyncTargetConfig(AsyncRequestsShowMegarepoSyncTargetConfigArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let fb = app.environment().fb;
    let session = SessionContainer::new_with_defaults(fb);
    let ctx = session.new_context(app.environment().scuba_sample_builder.clone());

    match args.subcommand {
        AsyncRequestsSubcommand::List(list_args) => {
            let repo_ids = match list_args.repo.as_repo_arg() {
                Some(repo_arg) => Some(vec![app.repo_id(repo_arg)?]),
                None => None,
            };
            let queue = async_requests_client::build(fb, &app, repo_ids)
                .await
                .context("acquiring the async requests queue")?;
            list::list_requests(list_args, ctx, queue).await?
        }
        AsyncRequestsSubcommand::FailDeadReadyRequests(fail_dead_ready_args) => {
            let queue = async_requests_client::build(fb, &app, None)
                .await
                .context("acquiring the async requests queue")?;
            fail_dead_ready::fail_dead_ready_requests(fail_dead_ready_args, ctx, queue).await?
        }
        AsyncRequestsSubcommand::Show(show_args) => {
            let queue = async_requests_client::build(fb, &app, None)
                .await
                .context("acquiring the async requests queue")?;
            let mononoke = Arc::new(
                app.open_managed_repo_arg::<Repo>(&show_args.repo)
                    .await
                    .context("Failed to initialize Mononoke API")?
                    .make_mononoke_api()?,
            );
            show::show_request(show_args, ctx, queue, mononoke).await?
        }
        AsyncRequestsSubcommand::Requeue(requeue_args) => {
            let queue = async_requests_client::build(fb, &app, None)
                .await
                .context("acquiring the async requests queue")?;
            requeue::requeue_request(requeue_args, ctx, queue).await?
        }
        AsyncRequestsSubcommand::Abort(abort_args) => {
            let queue = async_requests_client::build(fb, &app, None)
                .await
                .context("acquiring the async requests queue")?;
            abort::abort_request(abort_args, ctx, queue).await?
        }
        AsyncRequestsSubcommand::Submit(submit_args) => {
            let queue = async_requests_client::build(fb, &app, None)
                .await
                .context("acquiring the async requests queue")?;
            let repo = app.open_repo(&submit_args.repo).await?;
            submit::submit_request(submit_args, ctx, queue, repo).await?
        }
        AsyncRequestsSubcommand::ShowMegarepoSyncTargetConfig(config_args) => {
            let repo: Repo = app.open_repo(&config_args.repo).await?;
            let repo_id = repo.repo_identity.id();
            let repo_config = repo.repo_config_arc();
            show_megarepo_sync_target_config::show_megarepo_sync_target_config(
                config_args,
                ctx,
                &app,
                repo_id,
                repo_config,
            )
            .await?
        }
    }
    Ok(())
}
