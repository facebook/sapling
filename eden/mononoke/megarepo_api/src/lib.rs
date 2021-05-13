/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::{bail, Error};
use async_once_cell::AsyncOnceCell;
use async_requests::AsyncMethodRequestQueue;
use blobstore::Blobstore;
use context::CoreContext;
use environment::MononokeEnvironment;
use megarepo_config::{
    CfgrMononokeMegarepoConfigs, MononokeMegarepoConfigs, MononokeMegarepoConfigsOptions, Target,
    TestMononokeMegarepoConfigs,
};
use megarepo_error::MegarepoError;
use megarepo_mapping::MegarepoMapping;
use metaconfig_parser::RepoConfigs;
use metaconfig_types::ArcRepoConfig;
use mononoke_api::Mononoke;
use mononoke_types::RepositoryId;
use parking_lot::Mutex;
use repo_factory::RepoFactory;
use repo_identity::ArcRepoIdentity;
use requests_table::LongRunningRequestsQueue;
use slog::{info, o, warn};
use std::collections::HashMap;
use std::convert::TryFrom;
use std::future::Future;
use std::hash::Hash;
use std::sync::Arc;

/// A cache for AsyncMethodRequestQueue instances
#[derive(Clone)]
struct Cache<K: Clone + Eq + Hash, V: Clone> {
    cache: Arc<Mutex<HashMap<K, Arc<AsyncOnceCell<V>>>>>,
}

impl<K: Clone + Eq + Hash, V: Clone> Cache<K, V> {
    fn new() -> Self {
        Cache {
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn get_or_try_init<F, Fut>(&self, key: &K, init: F) -> Result<V, Error>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<V, Error>>,
    {
        let cell = {
            let mut cache = self.cache.lock();
            match cache.get(key) {
                Some(cell) => {
                    if let Some(value) = cell.get() {
                        return Ok(value.clone());
                    }
                    cell.clone()
                }
                None => {
                    let cell = Arc::new(AsyncOnceCell::new());
                    cache.insert(key.clone(), cell.clone());
                    cell
                }
            }
        };
        let value = cell.get_or_try_init(init).await?;
        Ok(value.clone())
    }
}

#[derive(Clone)]
pub struct MegarepoApi {
    megarepo_configs: Arc<dyn MononokeMegarepoConfigs>,
    repo_configs: RepoConfigs,
    repo_factory: RepoFactory,
    queue_cache: Cache<ArcRepoIdentity, AsyncMethodRequestQueue>,
    megarepo_mapping_cache: Cache<ArcRepoIdentity, Arc<MegarepoMapping>>,
    mononoke: Arc<Mononoke>,
}

impl MegarepoApi {
    pub async fn new(
        env: &Arc<MononokeEnvironment>,
        repo_configs: RepoConfigs,
        repo_factory: RepoFactory,
        mononoke: Arc<Mononoke>,
    ) -> Result<Self, MegarepoError> {
        let fb = env.fb;
        let logger = env.logger.new(o!("megarepo" => ""));

        let config_store = env.config_store.clone();
        let megarepo_configs: Arc<dyn MononokeMegarepoConfigs> = match env.megarepo_configs_options
        {
            MononokeMegarepoConfigsOptions::Prod => {
                Arc::new(CfgrMononokeMegarepoConfigs::new(fb, &logger, config_store)?)
            }
            MononokeMegarepoConfigsOptions::Test => {
                Arc::new(TestMononokeMegarepoConfigs::new(&logger))
            }
        };

        Ok(Self {
            megarepo_configs,
            repo_configs,
            repo_factory,
            queue_cache: Cache::new(),
            megarepo_mapping_cache: Cache::new(),
            mononoke,
        })
    }

    /// Get megarepo configs
    pub fn configs(&self) -> Arc<dyn MononokeMegarepoConfigs> {
        self.megarepo_configs.clone()
    }

    /// Get an `AsyncMethodRequestQueue`
    pub async fn async_method_request_queue(
        &self,
        ctx: &CoreContext,
        target: &Target,
    ) -> Result<AsyncMethodRequestQueue, MegarepoError> {
        let (repo_config, repo_identity) = self.target_repo(ctx, target).await?;

        let queue = self
            .queue_cache
            .get_or_try_init(&repo_identity.clone(), || async move {
                let table = self
                    .requests_table(ctx, &repo_identity, &repo_config)
                    .await?;
                let blobstore = self.blobstore(ctx, repo_identity, &repo_config).await?;
                Ok(AsyncMethodRequestQueue::new(table, blobstore))
            })
            .await?;
        Ok(queue)
    }

    /// Build an instance of `LongRunningRequestsQueue` to be embedded
    /// into `AsyncMethodRequestQueue`.
    async fn requests_table(
        &self,
        ctx: &CoreContext,
        repo_identity: &ArcRepoIdentity,
        repo_config: &ArcRepoConfig,
    ) -> Result<Arc<dyn LongRunningRequestsQueue>, Error> {
        info!(
            ctx.logger(),
            "Opening a long_running_requests_queue table for {}",
            repo_identity.name()
        );
        let table = self
            .repo_factory
            .long_running_requests_queue(&repo_config)
            .await?;
        info!(
            ctx.logger(),
            "Done opening a long_running_requests_queue table for {}",
            repo_identity.name()
        );

        Ok(table)
    }

    /// Get Mononoke repo config and identity by a target
    async fn target_repo(
        &self,
        ctx: &CoreContext,
        target: &Target,
    ) -> Result<(ArcRepoConfig, ArcRepoIdentity), Error> {
        let repo_id: i32 = TryFrom::<i64>::try_from(target.repo_id)?;
        let repo_id = RepositoryId::new(repo_id);
        let (name, cfg) = match self.repo_configs.get_repo_config(repo_id) {
            Some((name, cfg)) => (name, cfg),
            None => {
                warn!(
                    ctx.logger(),
                    "Unknown repo: {} in target {:?}", repo_id, target
                );
                bail!("unknown repo in the target: {}", repo_id)
            }
        };

        let cfg = self.repo_factory.repo_config(cfg);
        let repo_identity = self.repo_factory.repo_identity(name, &cfg);
        Ok((cfg, repo_identity))
    }

    /// Build a blobstore to be embedded into `AsyncMethodRequestQueue`
    async fn blobstore(
        &self,
        ctx: &CoreContext,
        repo_identity: ArcRepoIdentity,
        repo_config: &ArcRepoConfig,
    ) -> Result<Arc<dyn Blobstore>, Error> {
        info!(
            ctx.logger(),
            "Instantiating a MegarepoApi blobstore for {}",
            repo_identity.name()
        );
        let repo_blobstore = self
            .repo_factory
            .repo_blobstore(&repo_identity, &repo_config)
            .await?;
        let blobstore = repo_blobstore.boxed();
        info!(
            ctx.logger(),
            "Done instantiating a MegarepoApi blobstore for {}",
            repo_identity.name()
        );
        Ok(blobstore)
    }

    /// Build MegarepoMapping
    #[allow(unused)]
    async fn megarepo_mapping(
        &self,
        ctx: &CoreContext,
        target: &Target,
    ) -> Result<Arc<MegarepoMapping>, Error> {
        let (repo_config, repo_identity) = self.target_repo(ctx, target).await?;

        let megarepo_mapping = self
            .megarepo_mapping_cache
            .get_or_try_init(&repo_identity.clone(), || async move {
                let megarepo_mapping = self
                    .repo_factory
                    .sql_factory(&repo_config.storage_config.metadata)
                    .await?
                    .open::<MegarepoMapping>()?;

                Ok(Arc::new(megarepo_mapping))
            })
            .await?;

        Ok(megarepo_mapping)
    }
}
