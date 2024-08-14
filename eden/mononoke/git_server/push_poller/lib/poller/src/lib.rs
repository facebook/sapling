/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Error;
use anyhow::Result;
use cached_config::ConfigStore;
use clap::Parser;
use context::CoreContext;
use ephemeral_shard::EphemeralSchema;
use fbinit::FacebookInit;
use futures::prelude::*;
use git_push_redirect::GitPushRedirectConfig;
use git_push_redirect::SqlGitPushRedirectConfigBuilder;
use git_push_redirect::Staleness;
use metaconfig_parser::load_empty_repo_configs;
use metaconfig_parser::RepoConfigs;
use mysql_client::ConnectionOptionsBuilder;
use mysql_client::ConnectionPoolOptionsBuilder;
use repository::Repository;
use slog::Logger;
use sql_construct::SqlConstruct;
use storage::Destination;
use storage::Xdb;
use storage::XdbFactory;
use tokio::time::Duration;

const MONONOKE_PRODUCTION_SHARD_NAME: &str = "xdb.mononoke_production";
const METAGIT_SHARD_NAME: &str = "xdb.metagit";

#[derive(Debug, Parser)]
pub struct Args {
    /// Seconds between checking for new updates to Mononoke Git repositories.
    #[arg(long = "mononoke-polling-interval", default_value = "5")]
    mononoke_polling_interval: u64,
    /// Path to the Mononoke configs.
    #[arg(
        long = "mononoke-config-path",
        default_value = "configerator://scm/mononoke/repos/tiers/scs"
    )]
    mononoke_config_path: String,
    /// Maximum concurrency of operations during one iteration of polling.
    #[arg(long = "concurrency", default_value = "10")]
    concurrency: usize,
}

/// Struct providing access to the Source of Truth information for Git repositories.
pub struct GitSourceOfTruth {
    ctx: CoreContext,
    repo_configs: RepoConfigs,
    mononoke_production_xdb: Arc<Xdb>,
    metagit_xdb: Arc<Xdb>,
}

impl GitSourceOfTruth {
    pub async fn new(fb: FacebookInit, logger: Logger, config_path: &String) -> Result<Self> {
        let ctx = CoreContext::new_with_logger(fb, logger.clone());
        let config_store = create_config_store(fb, logger.clone())?;
        let repo_configs = metaconfig_parser::load_repo_configs(config_path, &config_store)?;
        let xdb_factory = create_prod_xdb_factory(fb)?;
        let mononoke_production_xdb = xdb_factory
            .create_or_get_shard(MONONOKE_PRODUCTION_SHARD_NAME)
            .await?;
        let metagit_xdb = xdb_factory.create_or_get_shard(METAGIT_SHARD_NAME).await?;
        Ok(Self {
            ctx,
            repo_configs,
            mononoke_production_xdb,
            metagit_xdb,
        })
    }

    pub async fn new_test(fb: FacebookInit) -> Self {
        let ctx = CoreContext::test_mock(fb);
        let repo_configs = load_empty_repo_configs();
        let xdb_factory = create_ephemeral_xdb_factory(fb).unwrap();
        let mononoke_production_xdb = xdb_factory
            .create_or_get_shard(MONONOKE_PRODUCTION_SHARD_NAME)
            .await
            .unwrap();
        let metagit_xdb = xdb_factory
            .create_or_get_shard(METAGIT_SHARD_NAME)
            .await
            .unwrap();
        Self {
            ctx,
            repo_configs,
            mononoke_production_xdb,
            metagit_xdb,
        }
    }

    pub async fn mononoke_source_of_truth(
        &self,
        repo_name: &str,
        staleness: Staleness,
    ) -> Result<bool> {
        let connections = self.mononoke_production_xdb.read_conns().await?;
        let git_push_redirect_config: &dyn GitPushRedirectConfig =
            &SqlGitPushRedirectConfigBuilder::from_sql_connections(connections).build();
        let maybe_repo_id = self
            .repo_configs
            .repos
            .get(repo_name)
            .map(|repo_config| repo_config.repoid);
        // If the repo is not in the config, we assume it is not a Mononoke Git repository.
        if let Some(repo_id) = maybe_repo_id {
            git_push_redirect_config
                .get_by_repo_id(&self.ctx, repo_id, staleness)
                .await
                .map(|entry| entry.map_or(false, |entry| entry.mononoke))
        } else {
            Ok(false)
        }
    }

    async fn current_mononoke_git_repositories<'a>(&'a self) -> Result<Vec<Repository<'a>>> {
        let connections = self.mononoke_production_xdb.read_conns().await?;
        let git_push_redirect_config: &dyn GitPushRedirectConfig =
            &SqlGitPushRedirectConfigBuilder::from_sql_connections(connections).build();

        let current_mononoke_git_repository_ids: HashSet<_> = git_push_redirect_config
            .get_redirected_to_mononoke(&self.ctx)
            .await?
            .into_iter()
            .map(|entry| entry.repo_id)
            .collect();

        let repositories: Vec<_> = self
            .repo_configs
            .repos
            .iter()
            .filter_map(|(name, repo_config)| {
                if current_mononoke_git_repository_ids.contains(&repo_config.repoid) {
                    Some(Repository::new(
                        repo_config.repoid,
                        name.to_string().into(),
                        &self.metagit_xdb,
                    ))
                } else {
                    None
                }
            })
            .collect();

        Ok(repositories)
    }

    async fn update_fingerprints(&self, concurrency: usize) -> Result<()> {
        let repositories = self.current_mononoke_git_repositories().await?;
        futures::stream::iter(repositories.into_iter().map(|repository| async move {
            repository.update_metagit_fingerprint().await?;
            Ok(repository)
        }))
        .buffer_unordered(concurrency)
        .for_each(|repository: Result<Repository>| async move {
            match repository {
                Ok(repository) => {
                    logging::info!("Successfully processed repository `{}`", repository.name())
                }
                Err(e) => logging::warn!("Failed to process a repository with error `{}`", e),
            }
        })
        .await;

        Ok(())
    }
}

pub fn create_config_store(fb: FacebookInit, logger: Logger) -> Result<ConfigStore> {
    const CRYPTO_PROJECT: &str = "SCM";
    const CONFIGERATOR_POLL_INTERVAL: Duration = Duration::from_secs(1);
    const CONFIGERATOR_REFRESH_TIMEOUT: Duration = Duration::from_secs(1);

    let crypto_regex_paths = vec!["scm/mononoke/repos/.*".to_string()];
    let crypto_regex = crypto_regex_paths
        .into_iter()
        .map(|path| (path, CRYPTO_PROJECT.to_string()))
        .collect();
    ConfigStore::regex_signed_configerator(
        fb,
        logger,
        crypto_regex,
        CONFIGERATOR_POLL_INTERVAL,
        CONFIGERATOR_REFRESH_TIMEOUT,
    )
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

fn create_ephemeral_xdb_factory(fb: FacebookInit) -> Result<XdbFactory> {
    let pool_options = ConnectionPoolOptionsBuilder::default()
        .build()
        .map_err(Error::msg)?;
    let conn_options = ConnectionOptionsBuilder::default()
        .build()
        .map_err(Error::msg)?;
    let destination = Destination::Ephemeral(EphemeralSchema::Live);
    XdbFactory::new(fb, destination, pool_options, conn_options)
}

pub async fn poll(fb: FacebookInit, args: Args) -> Result<()> {
    let logger = logging::get();
    let sot = GitSourceOfTruth::new(fb, logger.clone(), &args.mononoke_config_path).await?;

    let mut interval = tokio::time::interval(Duration::from_secs(args.mononoke_polling_interval));
    loop {
        interval.tick().await;
        if let Err(e) = sot.update_fingerprints(args.concurrency).await {
            logging::warn!(
                "Encounted error `{}` while updating fingerprints in iteration",
                e
            );
        }
    }
}
