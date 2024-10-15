/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::bail;
use anyhow::Error;
use async_requests::AsyncMethodRequestQueue;
use async_requests::AsyncRequestsError;
use blobstore::Blobstore;
use blobstore_factory::make_files_blobstore;
use blobstore_factory::make_manifold_blobstore;
use context::CoreContext;
use fbinit::FacebookInit;
use metaconfig_types::BlobConfig;
use mononoke_api::MononokeRepo;
use mononoke_api::RepositoryId;
use mononoke_app::MononokeApp;
use requests_table::LongRunningRequestsQueue;
use requests_table::SqlLongRunningRequestsQueue;
use slog::info;
use sql_construct::SqlConstructFromDatabaseConfig;
use sql_ext::facebook::MysqlOptions;

#[derive(Clone)]
pub struct AsyncRequestsQueue {
    sql_connection: Arc<dyn LongRunningRequestsQueue>,
    blobstore: Arc<dyn Blobstore>,
    repos: Option<Vec<RepositoryId>>,
}

impl AsyncRequestsQueue {
    /// Build a new async requests queue client. If the repos argument is specified,
    /// then the client will only be able to access the repos specified in the argument.
    pub async fn new(
        fb: FacebookInit,
        app: &MononokeApp,
        repos: Option<Vec<RepositoryId>>,
    ) -> Result<Self, Error> {
        let sql_connection = Arc::new(Self::open_sql_connection(fb, app).await?);
        let blobstore = Arc::new(Self::open_blobstore(fb, app).await?);

        Ok(Self {
            sql_connection,
            blobstore,
            repos,
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
            self.repos.clone(),
        ))
    }
}
