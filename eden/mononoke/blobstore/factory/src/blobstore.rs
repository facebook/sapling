/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Context, Error};
use blobstore::{Blobstore, DisabledBlob, ErrorKind};
use blobstore_sync_queue::SqlBlobstoreSyncQueue;
use chaosblob::{ChaosBlobstore, ChaosOptions};
use fbinit::FacebookInit;
use fileblob::Fileblob;
use futures::{
    compat::Future01CompatExt,
    future::{self, BoxFuture, Future, FutureExt},
};
use metaconfig_types::{
    BlobConfig, BlobstoreId, DatabaseConfig, MultiplexId, ScrubAction,
    ShardableRemoteDatabaseConfig,
};
use multiplexedblob::{LoggingScrubHandler, MultiplexedBlobstore, ScrubBlobstore, ScrubHandler};
use readonlyblob::ReadOnlyBlobstore;
use scuba::ScubaSampleBuilder;
use slog::Logger;
use sql_construct::SqlConstructFromDatabaseConfig;
use sql_ext::facebook::MysqlOptions;
use sqlblob::Sqlblob;
use std::num::NonZeroU64;
use std::sync::Arc;
use throttledblob::{ThrottleOptions, ThrottledBlob};

use crate::ReadOnlyStorage;

#[derive(Clone, Debug)]
pub struct BlobstoreOptions {
    pub chaos_options: ChaosOptions,
    pub throttle_options: ThrottleOptions,
    pub manifold_api_key: Option<String>,
}

impl BlobstoreOptions {
    pub fn new(
        chaos_options: ChaosOptions,
        throttle_options: ThrottleOptions,
        manifold_api_key: Option<String>,
    ) -> Self {
        Self {
            chaos_options,
            throttle_options,
            manifold_api_key,
        }
    }
}

impl Default for BlobstoreOptions {
    fn default() -> Self {
        Self::new(
            ChaosOptions::new(None, None),
            ThrottleOptions::new(None, None),
            None,
        )
    }
}

/// Construct a blobstore according to the specification. The multiplexed blobstore
/// needs an SQL DB for its queue, as does the MySQL blobstore.
/// If `throttling.read_qps` or `throttling.write_qps` are Some then ThrottledBlob will be used to limit
/// QPS to the underlying blobstore
pub fn make_blobstore<'a>(
    fb: FacebookInit,
    blobconfig: BlobConfig,
    mysql_options: MysqlOptions,
    readonly_storage: ReadOnlyStorage,
    blobstore_options: &'a BlobstoreOptions,
    logger: &'a Logger,
) -> BoxFuture<'a, Result<Arc<dyn Blobstore>, Error>> {
    // NOTE: This needs to return a BoxFuture because it recurses.
    async move {
        use BlobConfig::*;

        let mut has_components = false;
        let store = match blobconfig {
            Disabled => {
                Arc::new(DisabledBlob::new("Disabled by configuration")) as Arc<dyn Blobstore>
            }

            Files { path } => Fileblob::create(path.join("blobs"))
                .context(ErrorKind::StateOpen)
                .map(|store| Arc::new(store) as Arc<dyn Blobstore>)?,

            Sqlite { path } => Sqlblob::with_sqlite_path(path.join("blobs"), readonly_storage.0)
                .context(ErrorKind::StateOpen)
                .map(|store| Arc::new(store) as Arc<dyn Blobstore>)?,

            Mysql { remote } => match remote {
                ShardableRemoteDatabaseConfig::Unsharded(config) => {
                    if let Some(myrouter_port) = mysql_options.myrouter_port {
                        Sqlblob::with_myrouter_unsharded(
                            fb,
                            config.db_address,
                            myrouter_port,
                            mysql_options.read_connection_type(),
                            readonly_storage.0,
                        )
                        .compat()
                        .await
                    } else {
                        Sqlblob::with_raw_xdb_unsharded(
                            fb,
                            config.db_address,
                            mysql_options.read_connection_type(),
                            readonly_storage.0,
                        )
                        .compat()
                        .await
                    }
                }
                ShardableRemoteDatabaseConfig::Sharded(config) => {
                    if let Some(myrouter_port) = mysql_options.myrouter_port {
                        Sqlblob::with_myrouter(
                            fb,
                            config.shard_map.clone(),
                            myrouter_port,
                            mysql_options.read_connection_type(),
                            config.shard_num,
                            readonly_storage.0,
                        )
                        .compat()
                        .await
                    } else {
                        Sqlblob::with_raw_xdb_shardmap(
                            fb,
                            config.shard_map.clone(),
                            mysql_options.read_connection_type(),
                            config.shard_num,
                            readonly_storage.0,
                        )
                        .compat()
                        .await
                    }
                }
            }
            .map(|store| Arc::new(store) as Arc<dyn Blobstore>)?,
            Multiplexed {
                multiplex_id,
                scuba_table,
                scuba_sample_rate,
                blobstores,
                queue_db,
            } => {
                has_components = true;
                make_blobstore_multiplexed(
                    fb,
                    multiplex_id,
                    queue_db,
                    scuba_table,
                    scuba_sample_rate,
                    blobstores,
                    None,
                    mysql_options,
                    readonly_storage,
                    blobstore_options,
                    logger,
                )
                .await?
            }
            Scrub {
                multiplex_id,
                scuba_table,
                scuba_sample_rate,
                blobstores,
                scrub_action,
                queue_db,
            } => {
                has_components = true;
                make_blobstore_multiplexed(
                    fb,
                    multiplex_id,
                    queue_db,
                    scuba_table,
                    scuba_sample_rate,
                    blobstores,
                    Some((
                        Arc::new(LoggingScrubHandler::new(false)) as Arc<dyn ScrubHandler>,
                        scrub_action,
                    )),
                    mysql_options,
                    readonly_storage,
                    blobstore_options,
                    logger,
                )
                .await?
            }

            Manifold { bucket, prefix } => {
                #[cfg(fbcode_build)]
                {
                    crate::facebook::make_manifold_blobstore(
                        fb,
                        prefix.clone(),
                        bucket.clone(),
                        None,
                        blobstore_options.manifold_api_key.clone(),
                    )
                    .compat()
                    .await?
                }
                #[cfg(not(fbcode_build))]
                {
                    let _ = (bucket, prefix);
                    unimplemented!("This is implemented only for fbcode_build")
                }
            }
            ManifoldWithTtl {
                bucket,
                prefix,
                ttl,
            } => {
                #[cfg(fbcode_build)]
                {
                    crate::facebook::make_manifold_blobstore(
                        fb,
                        prefix.clone(),
                        bucket.clone(),
                        Some(ttl),
                        blobstore_options.manifold_api_key.clone(),
                    )
                    .compat()
                    .await?
                }
                #[cfg(not(fbcode_build))]
                {
                    let _ = (bucket, prefix, ttl);
                    unimplemented!("This is implemented only for fbcode_build")
                }
            }
        };

        let store = if readonly_storage.0 {
            Arc::new(ReadOnlyBlobstore::new(store)) as Arc<dyn Blobstore>
        } else {
            store
        };

        let store = if blobstore_options.throttle_options.has_throttle() {
            Arc::new(ThrottledBlob::new(
                store,
                blobstore_options.throttle_options.clone(),
            )) as Arc<dyn Blobstore>
        } else {
            store
        };

        // For stores with components only set chaos on their components
        let store = if !has_components && blobstore_options.chaos_options.has_chaos() {
            Arc::new(ChaosBlobstore::new(store, blobstore_options.chaos_options))
                as Arc<dyn Blobstore>
        } else {
            store
        };

        // NOTE: Do not add wrappers here that should only be added once per repository, since this
        // function will get called recursively for each member of a Multiplex! For those, use
        // RepoBlobstoreArgs::new instead.

        Ok(store)
    }
    .boxed()
}

pub fn make_blobstore_multiplexed<'a>(
    fb: FacebookInit,
    multiplex_id: MultiplexId,
    queue_db: DatabaseConfig,
    scuba_table: Option<String>,
    scuba_sample_rate: NonZeroU64,
    inner_config: Vec<(BlobstoreId, BlobConfig)>,
    scrub_args: Option<(Arc<dyn ScrubHandler>, ScrubAction)>,
    mysql_options: MysqlOptions,
    readonly_storage: ReadOnlyStorage,
    blobstore_options: &'a BlobstoreOptions,
    logger: &'a Logger,
) -> impl Future<Output = Result<Arc<dyn Blobstore>, Error>> + 'a {
    async move {
        let component_readonly = match &scrub_args {
            // Need to write to components to repair them.
            Some((_, ScrubAction::Repair)) => ReadOnlyStorage(false),
            _ => readonly_storage,
        };

        let mut applied_chaos = false;

        let components = future::try_join_all(inner_config.into_iter().map({
            move |(blobstoreid, config)| {
                let mut blobstore_options = blobstore_options.clone();

                if blobstore_options.chaos_options.has_chaos() {
                    if applied_chaos {
                        blobstore_options = BlobstoreOptions {
                            chaos_options: ChaosOptions::new(None, None),
                            ..blobstore_options
                        };
                    } else {
                        applied_chaos = true;
                    }
                }

                async move {
                    let store = make_blobstore(
                        fb,
                        config,
                        mysql_options,
                        component_readonly,
                        &blobstore_options,
                        logger,
                    )
                    .await?;

                    Ok((blobstoreid, store))
                }
            }
        }));

        let queue = SqlBlobstoreSyncQueue::with_database_config(
            fb,
            &queue_db,
            mysql_options,
            readonly_storage.0,
        );

        let (components, queue) = future::try_join(components, queue).await?;

        let blobstore = match scrub_args {
            Some((scrub_handler, scrub_action)) => Arc::new(ScrubBlobstore::new(
                multiplex_id,
                components,
                Arc::new(queue),
                scuba_table.map_or(ScubaSampleBuilder::with_discard(), |table| {
                    ScubaSampleBuilder::new(fb, table)
                }),
                scuba_sample_rate,
                scrub_handler,
                scrub_action,
            )) as Arc<dyn Blobstore>,
            None => Arc::new(MultiplexedBlobstore::new(
                multiplex_id,
                components,
                Arc::new(queue),
                scuba_table.map_or(ScubaSampleBuilder::with_discard(), |table| {
                    ScubaSampleBuilder::new(fb, table)
                }),
                scuba_sample_rate,
            )) as Arc<dyn Blobstore>,
        };

        Ok(blobstore)
    }
}
