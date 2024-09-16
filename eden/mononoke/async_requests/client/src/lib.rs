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
use blobstore_factory::make_files_blobstore;
use blobstore_factory::make_manifold_blobstore;
use context::CoreContext;
use fbinit::FacebookInit;
use metaconfig_types::BlobConfig;
use metaconfig_types::RepoConfigArc;
use mononoke_api::Mononoke;
use mononoke_api::MononokeRepo;
use mononoke_app::MononokeApp;
use parking_lot::Mutex;
use repo_blobstore::RepoBlobstoreArc;
use repo_identity::ArcRepoIdentity;
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
#[allow(dead_code)]
struct Cache<K: Clone + Eq + Hash, V: Clone> {
    cache: Arc<Mutex<HashMap<K, Arc<AsyncOnceCell<V>>>>>,
}

#[allow(dead_code)]
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
#[allow(dead_code)]
pub struct AsyncRequestsQueue<R> {
    sql_connection: Arc<dyn LongRunningRequestsQueue>,
    blobstore: Arc<dyn Blobstore>,
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
        let sql_connection = Arc::new(Self::open_sql_connection(fb, app, &mononoke).await?);
        let blobstore = Arc::new(Self::open_blobstore(fb, app, &mononoke).await?);

        Ok(Self {
            sql_connection,
            blobstore,
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

    async fn open_blobstore(
        fb: FacebookInit,
        app: &MononokeApp,
        mononoke: &Arc<Mononoke<R>>,
    ) -> Result<Arc<dyn Blobstore>, Error> {
        let use_common_config = justknobs::eval(
            "scm/mononoke:async_requests_blobstore_from_common_config",
            None,
            None,
        )
        .unwrap_or(false);
        let use_legacy_config = justknobs::eval(
            "scm/mononoke:async_requests_blobstore_from_legacy_config",
            None,
            None,
        )
        .unwrap_or(true);

        let config = app.repo_configs().common.async_requests_config.clone();
        if use_common_config {
            if let Some(config) = config.blobstore {
                info!(
                    app.logger(),
                    "Initializing async_requests with an explicit config for the blobstore"
                );
                let options = app.blobstore_options();
                return match config {
                    BlobConfig::Manifold { .. } => {
                        make_manifold_blobstore(fb, config, options).await
                    }
                    BlobConfig::Files { .. } => make_files_blobstore(config, options)
                        .await
                        .map(|store| Arc::new(store) as Arc<dyn Blobstore>),
                    _ => {
                        bail!("Unsupported blobstore type for async requests")
                    }
                };
            } else {
                warn!(
                    app.logger(),
                    "No blobstore config found in common config; falling back to repo config"
                );
            }
        }

        if use_legacy_config {
            let repo = mononoke.raw_repo(ASYNC_REQUESTS_REPO).ok_or_else(|| {
                AsyncRequestsError::request(anyhow!("repo not found {}", ASYNC_REQUESTS_REPO))
            })?;
            warn!(
                app.logger(),
                "Initializing async_requests blobstore falling back to the repo config for {}",
                ASYNC_REQUESTS_REPO,
            );
            return Ok(repo.repo_blobstore_arc());
        }

        bail!("No db config found in common config and legacy config is disabled")
    }

    /// Get the `AsyncMethodRequestQueue`
    pub async fn async_method_request_queue(
        &self,
        _ctx: &CoreContext,
    ) -> Result<AsyncMethodRequestQueue, AsyncRequestsError> {
        Ok(AsyncMethodRequestQueue::new(
            self.sql_connection.clone(),
            self.blobstore.clone(),
        ))
    }
}
