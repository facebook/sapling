/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{bail, Context, Error};
use blobstore::{
    Blobstore, BlobstorePutOps, DisabledBlob, ErrorKind, PutBehaviour, DEFAULT_PUT_BEHAVIOUR,
};
use blobstore_sync_queue::SqlBlobstoreSyncQueue;
use cacheblob::CachelibBlobstoreOptions;
use cached_config::ConfigStore;
use chaosblob::{ChaosBlobstore, ChaosOptions};
use fbinit::FacebookInit;
use fileblob::Fileblob;
use futures::{
    compat::Future01CompatExt,
    future::{self, BoxFuture, FutureExt},
};
use logblob::LogBlob;
use metaconfig_types::{
    BlobConfig, BlobstoreId, DatabaseConfig, MultiplexId, MultiplexedStoreType, ScrubAction,
    ShardableRemoteDatabaseConfig,
};
use multiplexedblob::{LoggingScrubHandler, MultiplexedBlobstore, ScrubBlobstore, ScrubHandler};
use packblob::{PackBlob, PackOptions};
use readonlyblob::ReadOnlyBlobstore;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::Logger;
use sql_construct::SqlConstructFromDatabaseConfig;
use sql_ext::facebook::{MysqlConnectionType, MysqlOptions};
use sqlblob::{CountedSqlblob, Sqlblob};
use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::Arc;
use throttledblob::{ThrottleOptions, ThrottledBlob};

use crate::ReadOnlyStorage;

#[derive(Clone, Debug)]
pub struct BlobstoreOptions {
    pub chaos_options: ChaosOptions,
    pub throttle_options: ThrottleOptions,
    pub manifold_api_key: Option<String>,
    pub pack_options: PackOptions,
    pub cachelib_options: CachelibBlobstoreOptions,
    pub put_behaviour: PutBehaviour,
}

impl BlobstoreOptions {
    pub fn new(
        chaos_options: ChaosOptions,
        throttle_options: ThrottleOptions,
        manifold_api_key: Option<String>,
        pack_options: PackOptions,
        cachelib_options: CachelibBlobstoreOptions,
        put_behaviour: Option<PutBehaviour>,
    ) -> Self {
        Self {
            chaos_options,
            throttle_options,
            manifold_api_key,
            pack_options,
            cachelib_options,
            // If not specified, maintain status quo, which is overwrite
            put_behaviour: put_behaviour.unwrap_or(DEFAULT_PUT_BEHAVIOUR),
        }
    }
}

impl Default for BlobstoreOptions {
    fn default() -> Self {
        Self::new(
            ChaosOptions::new(None, None),
            ThrottleOptions::new(None, None),
            None,
            PackOptions::default(),
            CachelibBlobstoreOptions::default(),
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
    config_store: &'a ConfigStore,
) -> BoxFuture<'a, Result<Arc<dyn Blobstore>, Error>> {
    async move {
        let store = make_blobstore_put_ops(
            fb,
            blobconfig,
            mysql_options,
            readonly_storage,
            blobstore_options,
            logger,
            config_store,
        )
        .await?;
        // Workaround for trait A {} trait B:A {} but Arc<dyn B> is not a Arc<dyn A>
        // See https://github.com/rust-lang/rfcs/issues/2765 if interested
        Ok(Arc::new(store) as Arc<dyn Blobstore>)
    }
    .boxed()
}

pub async fn make_sql_blobstore<'a>(
    fb: FacebookInit,
    blobconfig: BlobConfig,
    mysql_options: MysqlOptions,
    readonly_storage: ReadOnlyStorage,
    blobstore_options: &'a BlobstoreOptions,
    config_store: &'a ConfigStore,
) -> Result<CountedSqlblob, Error> {
    use BlobConfig::*;
    match blobconfig {
        Sqlite { path } => Sqlblob::with_sqlite_path(
            path.join("blobs"),
            readonly_storage.0,
            blobstore_options.put_behaviour,
            config_store,
        )
        .context(ErrorKind::StateOpen),

        Mysql { remote } => match remote {
            ShardableRemoteDatabaseConfig::Unsharded(config) => {
                let read_conn_type = mysql_options.read_connection_type();
                match mysql_options.connection_type {
                    MysqlConnectionType::Myrouter(myrouter_port) => {
                        Sqlblob::with_myrouter_unsharded(
                            fb,
                            config.db_address,
                            myrouter_port,
                            read_conn_type,
                            readonly_storage.0,
                            blobstore_options.put_behaviour,
                            config_store,
                        )
                        .compat()
                        .await
                    }
                    MysqlConnectionType::Mysql => {
                        Sqlblob::with_mysql_unsharded(
                            fb,
                            config.db_address,
                            read_conn_type,
                            readonly_storage.0,
                            blobstore_options.put_behaviour,
                            config_store,
                        )
                        .compat()
                        .await
                    }
                    MysqlConnectionType::RawXDB => {
                        Sqlblob::with_raw_xdb_unsharded(
                            fb,
                            config.db_address,
                            read_conn_type,
                            readonly_storage.0,
                            blobstore_options.put_behaviour,
                            config_store,
                        )
                        .compat()
                        .await
                    }
                }
            }
            ShardableRemoteDatabaseConfig::Sharded(config) => {
                let read_conn_type = mysql_options.read_connection_type();
                match mysql_options.connection_type {
                    MysqlConnectionType::Myrouter(myrouter_port) => {
                        Sqlblob::with_myrouter(
                            fb,
                            config.shard_map.clone(),
                            myrouter_port,
                            read_conn_type,
                            config.shard_num,
                            readonly_storage.0,
                            blobstore_options.put_behaviour,
                            config_store,
                        )
                        .compat()
                        .await
                    }
                    MysqlConnectionType::Mysql => {
                        Sqlblob::with_mysql(
                            fb,
                            config.shard_map.clone(),
                            config.shard_num,
                            read_conn_type,
                            readonly_storage.0,
                            blobstore_options.put_behaviour,
                            config_store,
                        )
                        .compat()
                        .await
                    }
                    MysqlConnectionType::RawXDB => {
                        Sqlblob::with_raw_xdb_shardmap(
                            fb,
                            config.shard_map.clone(),
                            read_conn_type,
                            config.shard_num,
                            readonly_storage.0,
                            blobstore_options.put_behaviour,
                            config_store,
                        )
                        .compat()
                        .await
                    }
                }
            }
        },
        _ => bail!("Not an SQL blobstore"),
    }
}

// Constructs the BlobstorePutOps store implementations for apps needing low level blobsore access
// most users should use `make_blobstore`
pub fn make_blobstore_put_ops<'a>(
    fb: FacebookInit,
    blobconfig: BlobConfig,
    mysql_options: MysqlOptions,
    readonly_storage: ReadOnlyStorage,
    blobstore_options: &'a BlobstoreOptions,
    logger: &'a Logger,
    config_store: &'a ConfigStore,
) -> BoxFuture<'a, Result<Arc<dyn BlobstorePutOps>, Error>> {
    // NOTE: This needs to return a BoxFuture because it recurses.
    async move {
        use BlobConfig::*;

        let mut has_components = false;
        let store = match blobconfig {
            Sqlite { .. } | Mysql { .. } => make_sql_blobstore(
                fb,
                blobconfig,
                mysql_options,
                readonly_storage,
                blobstore_options,
                config_store,
            )
            .await
            .map(|store| Arc::new(store) as Arc<dyn BlobstorePutOps>)?,

            Multiplexed {
                multiplex_id,
                scuba_table,
                scuba_sample_rate,
                blobstores,
                minimum_successful_writes,
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
                    minimum_successful_writes,
                    None,
                    mysql_options,
                    readonly_storage,
                    blobstore_options,
                    logger,
                    config_store,
                )
                .await?
            }
            Scrub {
                multiplex_id,
                scuba_table,
                scuba_sample_rate,
                blobstores,
                minimum_successful_writes,
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
                    minimum_successful_writes,
                    Some((
                        Arc::new(LoggingScrubHandler::new(false)) as Arc<dyn ScrubHandler>,
                        scrub_action,
                    )),
                    mysql_options,
                    readonly_storage,
                    blobstore_options,
                    logger,
                    config_store,
                )
                .await?
            }
            Disabled => {
                Arc::new(DisabledBlob::new("Disabled by configuration")) as Arc<dyn BlobstorePutOps>
            }

            Files { path } => Fileblob::create(path.join("blobs"), blobstore_options.put_behaviour)
                .context(ErrorKind::StateOpen)
                .map(|store| Arc::new(store) as Arc<dyn BlobstorePutOps>)?,

            Logging {
                blobconfig,
                scuba_table,
                scuba_sample_rate,
            } => {
                let scuba = scuba_table
                    .map_or(MononokeScubaSampleBuilder::with_discard(), |table| {
                        MononokeScubaSampleBuilder::new(fb, &table)
                    });

                let store = make_blobstore_put_ops(
                    fb,
                    *blobconfig,
                    mysql_options,
                    readonly_storage,
                    &blobstore_options,
                    logger,
                    config_store,
                )
                .await?;

                Arc::new(LogBlob::new(store, scuba, scuba_sample_rate)) as Arc<dyn BlobstorePutOps>
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
                        blobstore_options.put_behaviour,
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
                        blobstore_options.put_behaviour,
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
            Pack { blobconfig } => {
                let store = make_blobstore_put_ops(
                    fb,
                    *blobconfig,
                    mysql_options,
                    readonly_storage,
                    &blobstore_options,
                    logger,
                    config_store,
                )
                .await?;

                Arc::new(PackBlob::new(store, blobstore_options.pack_options.clone()))
                    as Arc<dyn BlobstorePutOps>
            }
            S3 {
                bucket,
                keychain_group,
                region_name,
                endpoint,
            } => {
                #[cfg(fbcode_build)]
                {
                    ::s3blob::S3Blob::new(
                        fb,
                        bucket,
                        keychain_group,
                        region_name,
                        endpoint,
                        blobstore_options.put_behaviour,
                        logger,
                    )
                    .await
                    .context(ErrorKind::StateOpen)
                    .map(|store| Arc::new(store) as Arc<dyn BlobstorePutOps>)?
                }
                #[cfg(not(fbcode_build))]
                {
                    let _ = (bucket, keychain_group, region_name, endpoint);
                    unimplemented!("This is implemented only for fbcode_build")
                }
            }
        };

        let store = if readonly_storage.0 {
            Arc::new(ReadOnlyBlobstore::new(store)) as Arc<dyn BlobstorePutOps>
        } else {
            store
        };

        let store = if blobstore_options.throttle_options.has_throttle() {
            Arc::new(ThrottledBlob::new(store, blobstore_options.throttle_options.clone()).await)
                as Arc<dyn BlobstorePutOps>
        } else {
            store
        };

        // For stores with components only set chaos on their components
        let store = if !has_components && blobstore_options.chaos_options.has_chaos() {
            Arc::new(ChaosBlobstore::new(store, blobstore_options.chaos_options))
                as Arc<dyn BlobstorePutOps>
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

pub async fn make_blobstore_multiplexed<'a>(
    fb: FacebookInit,
    multiplex_id: MultiplexId,
    queue_db: DatabaseConfig,
    scuba_table: Option<String>,
    scuba_sample_rate: NonZeroU64,
    inner_config: Vec<(BlobstoreId, MultiplexedStoreType, BlobConfig)>,
    minimum_successful_writes: NonZeroUsize,
    scrub_args: Option<(Arc<dyn ScrubHandler>, ScrubAction)>,
    mysql_options: MysqlOptions,
    readonly_storage: ReadOnlyStorage,
    blobstore_options: &'a BlobstoreOptions,
    logger: &'a Logger,
    config_store: &'a ConfigStore,
) -> Result<Arc<dyn BlobstorePutOps>, Error> {
    let component_readonly = match &scrub_args {
        // Need to write to components to repair them.
        Some((_, ScrubAction::Repair)) => ReadOnlyStorage(false),
        _ => readonly_storage,
    };

    let mut applied_chaos = false;

    let components = future::try_join_all(inner_config.into_iter().map({
        move |(blobstoreid, store_type, config)| {
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
                let store = make_blobstore_put_ops(
                    fb,
                    config,
                    mysql_options,
                    component_readonly,
                    &blobstore_options,
                    logger,
                    config_store,
                )
                .await?;

                Ok((blobstoreid, store_type, store))
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

    // For now, `partition` could do this, but this will be easier to extend when we introduce more store types
    let (normal_components, write_mostly_components) = {
        let mut normal_components = vec![];
        let mut write_mostly_components = vec![];
        for (blobstore_id, store_type, store) in components.into_iter() {
            match store_type {
                MultiplexedStoreType::Normal => normal_components.push((blobstore_id, store)),
                MultiplexedStoreType::WriteMostly => {
                    write_mostly_components.push((blobstore_id, store))
                }
            }
        }
        (normal_components, write_mostly_components)
    };

    let blobstore = match scrub_args {
        Some((scrub_handler, scrub_action)) => Arc::new(ScrubBlobstore::new(
            multiplex_id,
            normal_components,
            write_mostly_components,
            minimum_successful_writes,
            Arc::new(queue),
            scuba_table.map_or(MononokeScubaSampleBuilder::with_discard(), |table| {
                MononokeScubaSampleBuilder::new(fb, &table)
            }),
            scuba_sample_rate,
            scrub_handler,
            scrub_action,
        )) as Arc<dyn BlobstorePutOps>,
        None => Arc::new(MultiplexedBlobstore::new(
            multiplex_id,
            normal_components,
            write_mostly_components,
            minimum_successful_writes,
            Arc::new(queue),
            scuba_table.map_or(MononokeScubaSampleBuilder::with_discard(), |table| {
                MononokeScubaSampleBuilder::new(fb, &table)
            }),
            scuba_sample_rate,
        )) as Arc<dyn BlobstorePutOps>,
    };

    Ok(blobstore)
}
