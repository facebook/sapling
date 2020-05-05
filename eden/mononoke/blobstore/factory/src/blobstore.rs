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
use cloned::cloned;
use fbinit::FacebookInit;
use fileblob::Fileblob;
use futures::{FutureExt, TryFutureExt};
use futures_ext::{BoxFuture, FutureExt as _};
use futures_old::{
    future::{self, IntoFuture},
    Future,
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
pub fn make_blobstore(
    fb: FacebookInit,
    blobconfig: BlobConfig,
    mysql_options: MysqlOptions,
    readonly_storage: ReadOnlyStorage,
    blobstore_options: BlobstoreOptions,
    logger: Logger,
) -> BoxFuture<Arc<dyn Blobstore>, Error> {
    use BlobConfig::*;
    let mut has_components = false;
    let store = match blobconfig {
        Disabled => {
            Ok(Arc::new(DisabledBlob::new("Disabled by configuration")) as Arc<dyn Blobstore>)
                .into_future()
                .boxify()
        }

        Files { path } => Fileblob::create(path.join("blobs"))
            .context(ErrorKind::StateOpen)
            .map(|store| Arc::new(store) as Arc<dyn Blobstore>)
            .map_err(Error::from)
            .into_future()
            .boxify(),

        Sqlite { path } => Sqlblob::with_sqlite_path(path.join("blobs"), readonly_storage.0)
            .context(ErrorKind::StateOpen)
            .map_err(Error::from)
            .map(|store| Arc::new(store) as Arc<dyn Blobstore>)
            .into_future()
            .boxify(),

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
                } else {
                    Sqlblob::with_raw_xdb_unsharded(
                        fb,
                        config.db_address,
                        mysql_options.read_connection_type(),
                        readonly_storage.0,
                    )
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
                } else {
                    Sqlblob::with_raw_xdb_shardmap(
                        fb,
                        config.shard_map.clone(),
                        mysql_options.read_connection_type(),
                        config.shard_num,
                        readonly_storage.0,
                    )
                }
            }
        }
        .map(|store| Arc::new(store) as Arc<dyn Blobstore>)
        .into_future()
        .boxify(),
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
                mysql_options,
                readonly_storage,
                None,
                blobstore_options.clone(),
                logger,
            )
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
                mysql_options,
                readonly_storage,
                Some((
                    Arc::new(LoggingScrubHandler::new(false)) as Arc<dyn ScrubHandler>,
                    scrub_action,
                )),
                blobstore_options.clone(),
                logger,
            )
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
            }
            #[cfg(not(fbcode_build))]
            {
                let _ = (bucket, prefix, ttl);
                unimplemented!("This is implemented only for fbcode_build")
            }
        }
    };

    let store = if readonly_storage.0 {
        store
            .map(|inner| Arc::new(ReadOnlyBlobstore::new(inner)) as Arc<dyn Blobstore>)
            .boxify()
    } else {
        store
    };

    let store = if blobstore_options.throttle_options.has_throttle() {
        store
            .map({
                cloned!(blobstore_options);
                move |inner| {
                    Arc::new(ThrottledBlob::new(
                        inner,
                        blobstore_options.throttle_options.clone(),
                    )) as Arc<dyn Blobstore>
                }
            })
            .boxify()
    } else {
        store
    };

    // For stores with components only set chaos on their components
    let store = if !has_components && blobstore_options.chaos_options.has_chaos() {
        store
            .map(move |inner| {
                Arc::new(ChaosBlobstore::new(inner, blobstore_options.chaos_options))
                    as Arc<dyn Blobstore>
            })
            .boxify()
    } else {
        store
    };

    // NOTE: Do not add wrappers here that should only be added once per repository, since this
    // function will get called recursively for each member of a Multiplex! For those, use
    // RepoBlobstoreArgs::new instead.

    store
}

pub fn make_blobstore_multiplexed(
    fb: FacebookInit,
    multiplex_id: MultiplexId,
    queue_db: DatabaseConfig,
    scuba_table: Option<String>,
    scuba_sample_rate: NonZeroU64,
    inner_config: Vec<(BlobstoreId, BlobConfig)>,
    mysql_options: MysqlOptions,
    readonly_storage: ReadOnlyStorage,
    scrub_args: Option<(Arc<dyn ScrubHandler>, ScrubAction)>,
    blobstore_options: BlobstoreOptions,
    logger: Logger,
) -> BoxFuture<Arc<dyn Blobstore>, Error> {
    let component_readonly = match &scrub_args {
        // Need to write to components to repair them.
        Some((_, ScrubAction::Repair)) => ReadOnlyStorage(false),
        _ => readonly_storage,
    };

    let mut applied_chaos = false;
    let components: Vec<_> = inner_config
        .into_iter()
        .map({
            cloned!(logger);
            move |(blobstoreid, config)| {
                cloned!(blobstoreid, mut blobstore_options);
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
                make_blobstore(
                    // force per line for easier merges
                    fb,
                    config,
                    mysql_options,
                    component_readonly,
                    blobstore_options,
                    logger.clone(),
                )
                .map({ move |store| (blobstoreid, store) })
            }
        })
        .collect();

    let queue = {
        // FIXME: remove cloning and boxing when this crate is migrated to new futures
        cloned!(queue_db);
        async move {
            SqlBlobstoreSyncQueue::with_database_config(
                fb,
                &queue_db,
                mysql_options,
                readonly_storage.0,
            )
            .await
        }
    }
    .boxed()
    .compat();

    queue
        .and_then({
            move |queue| {
                future::join_all(components).map({
                    move |components| match scrub_args {
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
                        ))
                            as Arc<dyn Blobstore>,
                        None => Arc::new(MultiplexedBlobstore::new(
                            multiplex_id,
                            components,
                            Arc::new(queue),
                            scuba_table.map_or(ScubaSampleBuilder::with_discard(), |table| {
                                ScubaSampleBuilder::new(fb, table)
                            }),
                            scuba_sample_rate,
                        )) as Arc<dyn Blobstore>,
                    }
                })
            }
        })
        .boxify()
}
