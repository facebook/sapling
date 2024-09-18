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
use mononoke_api::Mononoke;
use mononoke_api::MononokeRepo;
use mononoke_app::MononokeApp;
use parking_lot::Mutex;
use repo_identity::ArcRepoIdentity;
use requests_table::LongRunningRequestsQueue;
use requests_table::SqlLongRunningRequestsQueue;
use slog::info;
use sql_construct::SqlConstructFromDatabaseConfig;
use sql_ext::facebook::MysqlOptions;

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
        let sql_connection = Arc::new(Self::open_sql_connection(fb, app).await?);
        let blobstore = Arc::new(Self::open_blobstore(fb, app).await?);

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
    ) -> Result<SqlLongRunningRequestsQueue, Error> {
        let config = app.repo_configs().common.async_requests_config.clone();
        if let Some(config) = config.db_config {
            info!(
                app.logger(),
                "Initializing async_requests with an explicit config"
            );
            SqlLongRunningRequestsQueue::with_database_config(
                fb,
                &config,
                &MysqlOptions::default(),
                false,
            )
        } else {
            bail!("async_requests config is missing");
        }
    }

    async fn open_blobstore(
        fb: FacebookInit,
        app: &MononokeApp,
    ) -> Result<Arc<dyn Blobstore>, Error> {
        let config = app.repo_configs().common.async_requests_config.clone();
        if let Some(config) = config.blobstore {
            let options = app.blobstore_options();
            match config {
                BlobConfig::Manifold { .. } => make_manifold_blobstore(fb, config, options).await,
                BlobConfig::Files { .. } => make_files_blobstore(config, options)
                    .await
                    .map(|store| Arc::new(store) as Arc<dyn Blobstore>),
                _ => {
                    bail!("Unsupported blobstore type for async requests")
                }
            }
        } else {
            bail!("async_requests config is missing");
        }
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
