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
use blobstore::Blobstore;
use blobstore_factory::make_files_blobstore;
use blobstore_factory::make_manifold_blobstore;
use fbinit::FacebookInit;
use metaconfig_types::BlobConfig;
use mononoke_api::MononokeRepo;
use mononoke_api::RepositoryId;
use mononoke_app::MononokeApp;
use requests_table::SqlLongRunningRequestsQueue;
use slog::debug;
use sql_construct::SqlConstructFromDatabaseConfig;
use sql_ext::facebook::MysqlOptions;

/// Build a new async requests queue client. If the repos argument is specified,
/// then the client will only be able to access the repos specified in the argument.
pub async fn build(
    fb: FacebookInit,
    app: &MononokeApp,
    repos: Option<Vec<RepositoryId>>,
) -> Result<AsyncMethodRequestQueue, Error> {
    let sql_connection = Arc::new(open_sql_connection(fb, app).await?);
    let blobstore = Arc::new(open_blobstore(fb, app).await?);

    Ok(AsyncMethodRequestQueue::new(
        sql_connection,
        blobstore,
        repos,
    ))
}

pub async fn open_sql_connection(
    fb: FacebookInit,
    app: &MononokeApp,
) -> Result<SqlLongRunningRequestsQueue, Error> {
    let config = app.repo_configs().common.async_requests_config.clone();
    if let Some(config) = config.db_config {
        debug!(
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

pub async fn open_blobstore(
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
