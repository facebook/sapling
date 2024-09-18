/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod abort;
mod list;
mod requeue;
mod show;
mod submit;

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use client::AsyncRequestsQueue;
use context::SessionContainer;
use mononoke_api::Repo;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use submit::AsyncRequestsSubmitArgs;

use crate::commands::async_requests::abort::AsyncRequestsAbortArgs;
use crate::commands::async_requests::list::AsyncRequestsListArgs;
use crate::commands::async_requests::requeue::AsyncRequestsRequeueArgs;
use crate::commands::async_requests::show::AsyncRequestsShowArgs;

/// View and manage the SCS async requests
#[derive(Parser)]
pub struct CommandArgs {
    /// The repository name or ID
    #[clap(flatten)]
    repo: RepoArgs,
    /// The subcommand for async requests
    #[clap(subcommand)]
    subcommand: AsyncRequestsSubcommand,
}

#[derive(Subcommand)]
pub enum AsyncRequestsSubcommand {
    /// Lists asynchronous requests (by default the ones active
    /// now or updated within last 5 mins).
    List(AsyncRequestsListArgs),
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
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();
    let mononoke = Arc::new(
        app.open_managed_repo_arg::<Repo>(&args.repo)
            .await
            .context("Failed to initialize Mononoke API")?
            .make_mononoke_api()?,
    );
    let queues_client: AsyncRequestsQueue = AsyncRequestsQueue::new(ctx.fb, &app, None)
        .await
        .context("acquiring the async requests queue")?;

    let session = SessionContainer::new_with_defaults(app.environment().fb);
    let ctx = session.new_context(
        app.logger().clone(),
        app.environment().scuba_sample_builder.clone(),
    );

    match args.subcommand {
        AsyncRequestsSubcommand::List(list_args) => {
            list::list_requests(list_args, ctx, queues_client).await?
        }
        AsyncRequestsSubcommand::Show(show_args) => {
            show::show_request(show_args, ctx, queues_client, mononoke).await?
        }
        AsyncRequestsSubcommand::Requeue(requeue_args) => {
            requeue::requeue_request(requeue_args, ctx, queues_client).await?
        }
        AsyncRequestsSubcommand::Abort(abort_args) => {
            abort::abort_request(abort_args, ctx, queues_client).await?
        }
        AsyncRequestsSubcommand::Submit(abort_args) => {
            let repo = app.open_repo(&args.repo).await?;
            submit::submit_request(abort_args, ctx, queues_client, mononoke, repo).await?
        }
    }
    Ok(())
}
