/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::Error;
use anyhow::Result;
use clap::Parser;
use context::CoreContext;
use fbinit::FacebookInit;
use logger::create_logger;
use mononoke_types::RepositoryId;
use mysql_client::ConnectionOptionsBuilder;
use mysql_client::ConnectionPoolOptionsBuilder;
use push_redirect_config::GitPushRedirectConfig;
use push_redirect_config::GitPushRedirectConfigEntry;
use push_redirect_config::SqlGitPushRedirectConfigBuilder;
use sql_construct::SqlConstruct;
use storage::Destination;
use storage::XdbFactory;
use tokio::time::Duration;

const MONONOKE_PRODUCTION_SHARD_NAME: &str = "xdb.mononoke_production";

#[derive(Debug, Parser)]
pub struct Args {
    /// Seconds between checking for new updates to Mononoke Git repositories.
    #[arg(long = "mononoke-polling-interval", default_value = "5")]
    mononoke_polling_interval: u64,
}

fn create_prod_xdb_factory(fb: FacebookInit) -> Result<XdbFactory> {
    let pool_options = ConnectionPoolOptionsBuilder::default()
        .build()
        .map_err(Error::msg)?;
    let conn_options = ConnectionOptionsBuilder::default()
        .build()
        .map_err(Error::msg)?;
    let destination = Destination::Prod;
    XdbFactory::new(fb, destination, pool_options, conn_options)
}

async fn current_mononoke_git_repositories(
    ctx: &CoreContext,
    xdb_factory: &XdbFactory,
) -> Result<Vec<RepositoryId>> {
    let xdb = xdb_factory
        .create_or_get_shard(MONONOKE_PRODUCTION_SHARD_NAME)
        .await?;
    let connections = xdb.read_conns().await?;
    let git_push_redirect_config: &dyn GitPushRedirectConfig =
        &SqlGitPushRedirectConfigBuilder::from_sql_connections(connections).build();

    let entries: Vec<GitPushRedirectConfigEntry> = git_push_redirect_config
        .get_redirected_to_mononoke(ctx)
        .await?;
    Ok(entries.into_iter().map(|entry| entry.repo_id).collect())
}

pub async fn poll(fb: FacebookInit, args: Args) -> Result<()> {
    let logger = create_logger();
    let ctx = CoreContext::new_with_logger(fb, logger);
    let xdb_factory = create_prod_xdb_factory(fb)?;

    let mut interval = tokio::time::interval(Duration::from_secs(args.mononoke_polling_interval));
    loop {
        interval.tick().await;
        println!(
            "Current Mononoke Git server repositories: {:?}",
            current_mononoke_git_repositories(&ctx, &xdb_factory).await?
        );
    }
}
