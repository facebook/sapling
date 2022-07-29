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

use anyhow::Context;
use anyhow::Result;

use context::SessionContainer;
use maplit::hashmap;
use megarepo_api::MegarepoApi;

use metaconfig_parser::RepoConfigs;
use mononoke_api::Mononoke;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use std::sync::Arc;

use clap::Parser;
use clap::Subcommand;

use crate::commands::async_requests::abort::AsyncRequestsAbortArgs;
use crate::commands::async_requests::list::AsyncRequestsListArgs;
use crate::commands::async_requests::requeue::AsyncRequestsRequeueArgs;
use crate::commands::async_requests::show::AsyncRequestsShowArgs;

/// View and manage the SCS async requests (used by megarepo)
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
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let app = Arc::new(app);
    let (repo_name, repo_config) = app.repo_config(args.repo.id_or_name()?)?;
    let repo_configs = RepoConfigs {
        repos: hashmap! {
            repo_name => repo_config
        },
        common: app.repo_configs().common.clone(),
    };
    let repo_factory = app.repo_factory();
    let mononoke = Arc::new(
        Mononoke::new(Arc::clone(&app))
            .await
            .context("Failed to initialize Mononoke API")?,
    );
    let megarepo = MegarepoApi::new(app.environment(), repo_configs, repo_factory, mononoke)
        .await
        .context("Failed to initialize MegarepoApi")?;
    let session = SessionContainer::new_with_defaults(app.environment().fb);
    let ctx = session.new_context(
        app.logger().clone(),
        app.environment().scuba_sample_builder.clone(),
    );

    match args.subcommand {
        AsyncRequestsSubcommand::List(list_args) => {
            list::list_requests(list_args, ctx, megarepo).await?
        }
        AsyncRequestsSubcommand::Show(show_args) => {
            show::show_request(show_args, ctx, megarepo).await?
        }
        AsyncRequestsSubcommand::Requeue(requeue_args) => {
            requeue::requeue_request(requeue_args, ctx, megarepo).await?
        }
        AsyncRequestsSubcommand::Abort(abort_args) => {
            abort::abort_request(abort_args, ctx, megarepo).await?
        }
    }
    Ok(())
}
