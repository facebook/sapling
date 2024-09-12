/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::future::Future;
use std::hash::Hash;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Error;
use async_once_cell::AsyncOnceCell;
use async_requests::AsyncMethodRequestQueue;
use async_requests::AsyncRequestsError;
use blobstore::Blobstore;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::future::try_join_all;
use megarepo_config::Target;
use metaconfig_types::ArcRepoConfig;
use metaconfig_types::RepoConfigArc;
use metaconfig_types::RepoConfigRef;
use mononoke_api::Mononoke;
use mononoke_api::MononokeRepo;
use mononoke_app::MononokeApp;
use mononoke_types::RepositoryId;
use parking_lot::Mutex;
use repo_blobstore::RepoBlobstoreArc;
use repo_identity::ArcRepoIdentity;
use repo_identity::RepoIdentityArc;
use repo_identity::RepoIdentityRef;
use requests_table::LongRunningRequestsQueue;
use requests_table::SqlLongRunningRequestsQueue;
use slog::info;
use slog::warn;
use sql_construct::SqlConstructFromDatabaseConfig;
use sql_ext::facebook::MysqlOptions;

// XXX keep using the traditional repo to find the dbconfig. We will move this to a more specific config soon.
const ASYNC_REQUESTS_REPO: &str = "aosp";

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
pub struct AsyncRequestsQueue<R> {
    sql_connection: Arc<dyn LongRunningRequestsQueue>,
    queue_cache: Cache<ArcRepoIdentity, AsyncMethodRequestQueue>,
    mononoke: Arc<Mononoke<R>>,
}

impl<R: MononokeRepo> AsyncRequestsQueue<R> {
    /// Creates a new tailer instance that's going to use provided megarepo API
    /// The name argument should uniquely identify tailer instance and will be put
    /// in the queue table so it's possible to find out which instance is working on
    /// a given task (for debugging purposes).
    pub async fn new(
        fb: FacebookInit,
        app: &MononokeApp,
        mononoke: Arc<Mononoke<R>>,
    ) -> Result<Self, Error> {
        let sql_connection = Self::open_sql_connection(fb, app, &mononoke).await?;

        Ok(Self {
            sql_connection: Arc::new(sql_connection),
            queue_cache: Cache::new(),
            mononoke,
        })
    }

    async fn open_sql_connection(
        fb: FacebookInit,
        app: &MononokeApp,
        mononoke: &Arc<Mononoke<R>>,
    ) -> Result<SqlLongRunningRequestsQueue, Error> {
        let use_common_config =
            justknobs::eval("scm/mononoke:async_requests_from_common_config", None, None)
                .unwrap_or(false);
        let use_legacy_config =
            justknobs::eval("scm/mononoke:async_requests_legacy_config", None, None)
                .unwrap_or(true);

        let config = app.repo_configs().common.async_requests_config.clone();
        if use_common_config {
            if let Some(config) = config.db_config {
                info!(
                    app.logger(),
                    "Initializing async_requests with an explicit config"
                );
                return SqlLongRunningRequestsQueue::with_database_config(
                    fb,
                    &config,
                    &MysqlOptions::default(),
                    false,
                );
            } else {
                warn!(
                    app.logger(),
                    "No db config found in common config; falling back to repo config"
                );
            }
        }

        if use_legacy_config {
            let repo_factory = app.repo_factory().clone();
            let repo = mononoke.raw_repo(ASYNC_REQUESTS_REPO).ok_or_else(|| {
                AsyncRequestsError::internal(anyhow!(
                    "could not find the default repo for async requests",
                ))
            })?;
            let repo_config = repo.repo_config_arc();
            warn!(
                app.logger(),
                "Initializing async_requests falling back to the repo config for {}",
                ASYNC_REQUESTS_REPO,
            );
            let sql_factory = repo_factory
                .sql_factory(&repo_config.storage_config.metadata)
                .await?;
            return sql_factory.open::<SqlLongRunningRequestsQueue>().await;
        }

        bail!("No db config found in common config and legacy config is disabled")
    }

    /// Get an `AsyncMethodRequestQueue` for a given target
    pub async fn async_method_request_queue(
        &self,
        ctx: &CoreContext,
        target: &Target,
    ) -> Result<AsyncMethodRequestQueue, AsyncRequestsError> {
        let (repo_config, repo_identity) = self.target_repo_config_and_id(ctx, target).await?;
        self.async_method_request_queue_for_repo(ctx, &repo_identity, &repo_config)
            .await
    }

    /// Get an `AsyncMethodRequestQueue` for a given repo
    pub async fn async_method_request_queue_for_repo(
        &self,
        _ctx: &CoreContext,
        repo_identity: &ArcRepoIdentity,
        _repo_config: &ArcRepoConfig,
    ) -> Result<AsyncMethodRequestQueue, AsyncRequestsError> {
        let queue = self
            .queue_cache
            .get_or_try_init(&repo_identity.clone(), || async move {
                let blobstore = self.blobstore(repo_identity.clone()).await?;
                Ok(AsyncMethodRequestQueue::new(
                    self.sql_connection.clone(),
                    blobstore,
                ))
            })
            .await?;
        Ok(queue)
    }

    /// Get all queues used by configured repos.
    pub async fn all_async_method_request_queues(
        &self,
        ctx: &CoreContext,
    ) -> Result<Vec<(Vec<RepositoryId>, AsyncMethodRequestQueue)>, AsyncRequestsError> {
        // TODO(mitrandir): instead of creating a queue per repo create a queue
        // per group of repos with exactly same storage configs. This way even with
        // a lot of repos we'll have few queues.
        let queues = try_join_all(self.mononoke.repos().map(|repo| {
            let repo_identity = repo.repo_identity().clone();
            let repo_id = repo_identity.id();
            let repo_config = repo.repo_config().clone();
            async move {
                let queue = self
                    .async_method_request_queue_for_repo(
                        ctx,
                        &Arc::new(repo_identity),
                        &Arc::new(repo_config),
                    )
                    .await?;
                Ok::<_, AsyncRequestsError>((vec![repo_id], queue))
            }
        }))
        .await?;
        Ok(queues)
    }

    /// Get Mononoke repo config and identity by a target
    async fn target_repo_config_and_id(
        &self,
        ctx: &CoreContext,
        target: &Target,
    ) -> Result<(ArcRepoConfig, ArcRepoIdentity), Error> {
        let repo_id: i32 = TryFrom::<i64>::try_from(target.repo_id)?;
        let repo = match self.mononoke.raw_repo_by_id(repo_id) {
            Some(repo) => repo,
            None => {
                warn!(
                    ctx.logger(),
                    "Unknown repo: {} in target {:?}", repo_id, target
                );
                bail!("unknown repo in the target: {}", repo_id)
            }
        };
        Ok((repo.repo_config_arc(), repo.repo_identity_arc()))
    }

    /// Build a blobstore to be embedded into `AsyncMethodRequestQueue`
    async fn blobstore(&self, repo_identity: ArcRepoIdentity) -> Result<Arc<dyn Blobstore>, Error> {
        let repo = self
            .mononoke
            .raw_repo(repo_identity.name())
            .ok_or_else(|| {
                AsyncRequestsError::request(anyhow!("repo not found {}", repo_identity.name()))
            })?;
        Ok(repo.repo_blobstore_arc())
    }
}
