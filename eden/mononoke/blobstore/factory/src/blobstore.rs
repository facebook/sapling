/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::NonZeroU64;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use anyhow::bail;
use anyhow::Context;
use anyhow::Error;
use blobstore::Blobstore;
use blobstore::BlobstoreEnumerableWithUnlink;
use blobstore::BlobstorePutOps;
use blobstore::BlobstoreUnlinkOps;
use blobstore::DisabledBlob;
use blobstore::ErrorKind;
use blobstore::PutBehaviour;
use blobstore::DEFAULT_PUT_BEHAVIOUR;
use blobstore_sync_queue::SqlBlobstoreSyncQueue;
use blobstore_sync_queue::SqlBlobstoreWal;
use cacheblob::CachelibBlobstoreOptions;
use cached_config::ConfigStore;
use chaosblob::ChaosBlobstore;
use chaosblob::ChaosOptions;
use delayblob::DelayOptions;
use delayblob::DelayedBlobstore;
use fbinit::FacebookInit;
use fileblob::Fileblob;
use futures::future;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures_watchdog::WatchdogExt;
use logblob::LogBlob;
use metaconfig_types::BlobConfig;
use metaconfig_types::BlobstoreId;
use metaconfig_types::DatabaseConfig;
use metaconfig_types::MultiplexId;
use metaconfig_types::MultiplexedStoreType;
use metaconfig_types::PackConfig;
use metaconfig_types::ShardableRemoteDatabaseConfig;
use metaconfig_types::ShardedDatabaseConfig;
use multiplexedblob::MultiplexedBlobstore;
use multiplexedblob::ScrubAction;
use multiplexedblob::ScrubBlobstore;
use multiplexedblob::ScrubHandler;
use multiplexedblob::ScrubOptions;
use multiplexedblob::SrubWriteOnly;
use multiplexedblob_wal::scrub::WalScrubBlobstore;
use multiplexedblob_wal::Scuba as WalScuba;
use multiplexedblob_wal::WalMultiplexedBlobstore;
use packblob::PackBlob;
use packblob::PackOptions;
use readonlyblob::ReadOnlyBlobstore;
use samplingblob::ComponentSamplingHandler;
use samplingblob::SamplingBlobstorePutOps;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::Logger;
use sql_construct::SqlConstructFromDatabaseConfig;
use sql_construct::SqlConstructFromShardedDatabaseConfig;
use sql_ext::facebook::MysqlOptions;
use sqlblob::CountedSqlblob;
use sqlblob::Sqlblob;
use throttledblob::ThrottleOptions;
use throttledblob::ThrottledBlob;

use crate::ReadOnlyStorage;

#[derive(Clone, Debug)]
pub struct BlobstoreOptions {
    pub chaos_options: ChaosOptions,
    pub delay_options: DelayOptions,
    pub throttle_options: ThrottleOptions,
    #[cfg(fbcode_build)]
    pub manifold_options: crate::facebook::ManifoldOptions,
    pub pack_options: PackOptions,
    pub cachelib_options: CachelibBlobstoreOptions,
    pub put_behaviour: PutBehaviour,
    pub scrub_options: Option<ScrubOptions>,
    pub sqlblob_mysql_options: MysqlOptions,
}

impl BlobstoreOptions {
    pub fn new(
        chaos_options: ChaosOptions,
        delay_options: DelayOptions,
        throttle_options: ThrottleOptions,
        #[cfg(fbcode_build)] manifold_options: crate::facebook::ManifoldOptions,
        pack_options: PackOptions,
        cachelib_options: CachelibBlobstoreOptions,
        put_behaviour: Option<PutBehaviour>,
        sqlblob_mysql_options: MysqlOptions,
    ) -> Self {
        Self {
            chaos_options,
            delay_options,
            throttle_options,
            #[cfg(fbcode_build)]
            manifold_options,
            pack_options,
            cachelib_options,
            // If not specified, maintain status quo, which is overwrite
            put_behaviour: put_behaviour.unwrap_or(DEFAULT_PUT_BEHAVIOUR),
            // These are added via the builder methods
            scrub_options: None,
            sqlblob_mysql_options,
        }
    }

    pub fn set_scrub_options(&mut self, scrub_options: ScrubOptions) {
        self.scrub_options = Some(scrub_options);
    }

    pub fn with_scrub_action(self, scrub_action: Option<ScrubAction>) -> Self {
        if let Some(scrub_action) = scrub_action {
            let mut scrub_options = self.scrub_options.unwrap_or_default();
            scrub_options.scrub_action = scrub_action;
            Self {
                scrub_options: Some(scrub_options),
                ..self
            }
        } else {
            self
        }
    }

    pub fn with_scrub_grace(self, scrub_grace: Option<u64>) -> Self {
        if let Some(mut scrub_options) = self.scrub_options {
            scrub_options.scrub_grace = scrub_grace.map(Duration::from_secs);
            Self {
                scrub_options: Some(scrub_options),
                ..self
            }
        } else {
            self
        }
    }

    pub fn with_scrub_action_on_missing_write_only(self, scrub_missing: SrubWriteOnly) -> Self {
        if let Some(mut scrub_options) = self.scrub_options {
            scrub_options.scrub_action_on_missing_write_only = scrub_missing;
            Self {
                scrub_options: Some(scrub_options),
                ..self
            }
        } else {
            self
        }
    }

    pub fn with_scrub_queue_peek_bound(self, queue_peek_bound_secs: u64) -> Self {
        if let Some(mut scrub_options) = self.scrub_options {
            scrub_options.queue_peek_bound = Duration::from_secs(queue_peek_bound_secs);
            Self {
                scrub_options: Some(scrub_options),
                ..self
            }
        } else {
            self
        }
    }
}

/// Construct a blobstore according to the specification. The multiplexed blobstore
/// needs an SQL DB for its queue, as does the MySQL blobstore.
/// If `throttling.read_qps` or `throttling.write_qps` are Some then ThrottledBlob will be used to limit
/// QPS to the underlying blobstore
pub fn make_blobstore<'a>(
    fb: FacebookInit,
    blobconfig: BlobConfig,
    mysql_options: &'a MysqlOptions,
    readonly_storage: ReadOnlyStorage,
    blobstore_options: &'a BlobstoreOptions,
    logger: &'a Logger,
    config_store: &'a ConfigStore,
    scrub_handler: &'a Arc<dyn ScrubHandler>,
    component_sampler: Option<&'a Arc<dyn ComponentSamplingHandler>>,
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
            scrub_handler,
            component_sampler,
            None,
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
        Mysql { remote } => {
            let (tier_name, shard_count) = match remote {
                ShardableRemoteDatabaseConfig::Unsharded(config) => (config.db_address, None),
                ShardableRemoteDatabaseConfig::Sharded(config) => {
                    (config.shard_map.clone(), Some(config.shard_num))
                }
            };
            make_sql_blobstore_xdb(
                fb,
                tier_name,
                shard_count,
                blobstore_options,
                readonly_storage,
                blobstore_options.put_behaviour,
                config_store,
            )
            .await
        }
        _ => bail!("Not an SQL blobstore"),
    }
}

// Most users should call `make_sql_blobstore` instead, however its useful to expose this to reduce duplication with benchmark tools.
pub async fn make_sql_blobstore_xdb<'a>(
    fb: FacebookInit,
    tier_name: String,
    shard_count: Option<NonZeroUsize>,
    blobstore_options: &'a BlobstoreOptions,
    readonly_storage: ReadOnlyStorage,
    put_behaviour: PutBehaviour,
    config_store: &'a ConfigStore,
) -> Result<CountedSqlblob, Error> {
    let mysql_options = blobstore_options.sqlblob_mysql_options.clone();
    match shard_count {
        None => {
            Sqlblob::with_mysql_unsharded(
                fb,
                tier_name,
                mysql_options,
                readonly_storage.0,
                put_behaviour,
                config_store,
            )
            .await
        }
        Some(shard_num) => {
            Sqlblob::with_mysql(
                fb,
                tier_name,
                shard_num,
                mysql_options,
                readonly_storage.0,
                put_behaviour,
                config_store,
            )
            .await
        }
    }
}

pub fn make_packblob_wrapper<'a, T>(
    pack_config: Option<PackConfig>,
    blobstore_options: &'a BlobstoreOptions,
    store: T,
) -> Result<PackBlob<T>, Error> {
    // Take the user specified option if provided, otherwise use the config
    let put_format = if let Some(put_format) = blobstore_options.pack_options.override_put_format {
        put_format
    } else {
        pack_config.map(|c| c.put_format).unwrap_or_default()
    };

    Ok(PackBlob::new(store, put_format))
}

/// Construct a PackBlob according to the spec; you are responsible for
/// finding a PackBlob config
pub async fn make_packblob<'a>(
    fb: FacebookInit,
    blobconfig: BlobConfig,
    readonly_storage: ReadOnlyStorage,
    blobstore_options: &'a BlobstoreOptions,
    logger: &'a Logger,
    config_store: &'a ConfigStore,
) -> Result<PackBlob<Arc<dyn BlobstoreUnlinkOps>>, Error> {
    if let BlobConfig::Pack {
        pack_config,
        blobconfig,
    } = blobconfig
    {
        let store = make_blobstore_with_link(
            fb,
            *blobconfig,
            readonly_storage,
            blobstore_options,
            logger,
            config_store,
        )
        .watched(logger)
        .await?;

        Ok(make_packblob_wrapper(
            pack_config,
            blobstore_options,
            store,
        )?)
    } else {
        bail!("Not a PackBlob")
    }
}

#[cfg(fbcode_build)]
async fn make_manifold_blobstore(
    fb: FacebookInit,
    blobconfig: BlobConfig,
    blobstore_options: &BlobstoreOptions,
) -> Result<Arc<dyn BlobstoreEnumerableWithUnlink>, Error> {
    use BlobConfig::*;
    let (bucket, prefix, ttl) = match blobconfig {
        Manifold { bucket, prefix } => (bucket, prefix, None),
        ManifoldWithTtl {
            bucket,
            prefix,
            ttl,
        } => (bucket, prefix, Some(ttl)),
        _ => bail!("Not a Manifold blobstore"),
    };
    crate::facebook::make_manifold_blobstore(
        fb,
        &prefix,
        &bucket,
        ttl,
        &blobstore_options.manifold_options,
        blobstore_options.put_behaviour,
    )
}

#[cfg(not(fbcode_build))]
async fn make_manifold_blobstore(
    _fb: FacebookInit,
    _blobconfig: BlobConfig,
    _blobstore_options: &BlobstoreOptions,
) -> Result<Arc<dyn BlobstoreEnumerableWithUnlink>, Error> {
    unimplemented!("This is implemented only for fbcode_build")
}

async fn make_files_blobstore(
    blobconfig: BlobConfig,
    blobstore_options: &BlobstoreOptions,
) -> Result<Fileblob, Error> {
    if let BlobConfig::Files { path } = blobconfig {
        Fileblob::create(path.join("blobs"), blobstore_options.put_behaviour)
            .context(ErrorKind::StateOpen)
    } else {
        bail!("Not a file blobstore")
    }
}

async fn make_blobstore_with_link<'a>(
    fb: FacebookInit,
    blobconfig: BlobConfig,
    readonly_storage: ReadOnlyStorage,
    blobstore_options: &'a BlobstoreOptions,
    logger: &'a Logger,
    config_store: &'a ConfigStore,
) -> Result<Arc<dyn BlobstoreUnlinkOps>, Error> {
    use BlobConfig::*;
    match blobconfig {
        Sqlite { .. } | Mysql { .. } => make_sql_blobstore(
            fb,
            blobconfig,
            readonly_storage,
            blobstore_options,
            config_store,
        )
        .watched(logger)
        .await
        .map(|store| Arc::new(store) as Arc<dyn BlobstoreUnlinkOps>),
        Manifold { .. } | ManifoldWithTtl { .. } => {
            make_manifold_blobstore(fb, blobconfig, blobstore_options)
                .watched(logger)
                .await
                .map(|store| Arc::new(store) as Arc<dyn BlobstoreUnlinkOps>)
        }
        Files { .. } => make_files_blobstore(blobconfig, blobstore_options)
            .await
            .map(|store| Arc::new(store) as Arc<dyn BlobstoreUnlinkOps>),
        _ => bail!("Not a physical blobstore"),
    }
}

// Constructs the BlobstoreEnumerableWithUnlink store implementations
// for blobstores. If the blobstore is a wrapper blobstore, the inner
// physical blobstore construction is delegated to another function
// and the result is wrapped up in this function.
pub async fn make_blobstore_enumerable_with_unlink<'a>(
    fb: FacebookInit,
    blobconfig: BlobConfig,
    blobstore_options: &'a BlobstoreOptions,
    logger: &'a Logger,
) -> Result<Arc<dyn BlobstoreEnumerableWithUnlink>, Error> {
    use BlobConfig::*;
    match blobconfig {
        Pack {
            pack_config,
            blobconfig,
        } => {
            let store =
                raw_blobstore_enumerable_with_unlink(fb, *blobconfig, blobstore_options, logger)
                    .watched(logger)
                    .await?;
            let pack_store = make_packblob_wrapper(pack_config, blobstore_options, store)?;
            Ok(Arc::new(pack_store) as Arc<dyn BlobstoreEnumerableWithUnlink>)
        }
        _ => raw_blobstore_enumerable_with_unlink(fb, blobconfig, blobstore_options, logger).await,
    }
}

// Constructs the raw BlobstoreEnumerableWithUnlink store implementations for low level
// blobstore access. The blobstore created is NOT a wrapper (e.g. PackBlob)
pub async fn raw_blobstore_enumerable_with_unlink<'a>(
    fb: FacebookInit,
    blobconfig: BlobConfig,
    blobstore_options: &'a BlobstoreOptions,
    logger: &'a Logger,
) -> Result<Arc<dyn BlobstoreEnumerableWithUnlink>, Error> {
    use BlobConfig::*;
    match blobconfig {
        Manifold { .. } | ManifoldWithTtl { .. } => {
            make_manifold_blobstore(fb, blobconfig, blobstore_options)
                .watched(logger)
                .await
        }
        Files { .. } => make_files_blobstore(blobconfig, blobstore_options)
            .await
            .map(|store| Arc::new(store) as Arc<dyn BlobstoreEnumerableWithUnlink>),
        _ => bail!("Not a physical blobstore that supports unlink + keysource + putops"),
    }
}

// Constructs the BlobstorePutOps store implementations for low level blobstore access
fn make_blobstore_put_ops<'a>(
    fb: FacebookInit,
    blobconfig: BlobConfig,
    mysql_options: &'a MysqlOptions,
    readonly_storage: ReadOnlyStorage,
    blobstore_options: &'a BlobstoreOptions,
    logger: &'a Logger,
    config_store: &'a ConfigStore,
    scrub_handler: &'a Arc<dyn ScrubHandler>,
    component_sampler: Option<&'a Arc<dyn ComponentSamplingHandler>>,
    blobstore_id: Option<BlobstoreId>,
) -> BoxFuture<'a, Result<Arc<dyn BlobstorePutOps>, Error>> {
    // NOTE: This needs to return a BoxFuture because it recurses.
    async move {
        use BlobConfig::*;

        let mut needs_wrappers = true;
        let store = match blobconfig {
            // Physical blobstores
            Sqlite { .. } | Mysql { .. } => make_sql_blobstore(
                fb,
                blobconfig,
                readonly_storage,
                blobstore_options,
                config_store,
            )
            .watched(logger)
            .await
            .map(|store| Arc::new(store) as Arc<dyn BlobstorePutOps>)?,
            Manifold { .. } | ManifoldWithTtl { .. } => {
                make_manifold_blobstore(fb, blobconfig, blobstore_options)
                    .watched(logger)
                    .await
                    .map(|store| Arc::new(store) as Arc<dyn BlobstorePutOps>)?
            }
            Files { .. } => make_files_blobstore(blobconfig, blobstore_options)
                .await
                .map(|store| Arc::new(store) as Arc<dyn BlobstorePutOps>)?,
            S3 {
                bucket,
                keychain_group,
                region_name,
                endpoint,
                num_concurrent_operations,
                secret_name,
            } => {
                #[cfg(fbcode_build)]
                {
                    ::s3blob::S3Blob::new(
                        fb,
                        bucket,
                        keychain_group,
                        secret_name,
                        region_name,
                        endpoint,
                        blobstore_options.put_behaviour,
                        logger,
                        num_concurrent_operations,
                    )
                    .watched(logger)
                    .await
                    .context(ErrorKind::StateOpen)
                    .map(|store| Arc::new(store) as Arc<dyn BlobstorePutOps>)?
                }
                #[cfg(not(fbcode_build))]
                {
                    let _ = (
                        bucket,
                        keychain_group,
                        secret_name,
                        region_name,
                        endpoint,
                        num_concurrent_operations,
                    );
                    unimplemented!("This is implemented only for fbcode_build")
                }
            }

            // Special case
            Disabled => {
                Arc::new(DisabledBlob::new("Disabled by configuration")) as Arc<dyn BlobstorePutOps>
            }

            // Wrapper blobstores
            Multiplexed {
                multiplex_id,
                scuba_table,
                multiplex_scuba_table,
                scuba_sample_rate,
                blobstores,
                minimum_successful_writes,
                not_present_read_quorum,
                queue_db,
            } => {
                needs_wrappers = false;
                make_blobstore_multiplexed(
                    fb,
                    multiplex_id,
                    queue_db,
                    scuba_table,
                    multiplex_scuba_table,
                    scuba_sample_rate,
                    blobstores,
                    minimum_successful_writes,
                    not_present_read_quorum,
                    mysql_options,
                    readonly_storage,
                    blobstore_options,
                    logger,
                    config_store,
                    scrub_handler,
                    component_sampler,
                )
                .watched(logger)
                .await?
            }
            MultiplexedWal {
                multiplex_id,
                blobstores,
                write_quorum,
                queue_db,
                inner_blobstores_scuba_table,
                multiplex_scuba_table,
                scuba_sample_rate,
            } => {
                needs_wrappers = false;
                make_multiplexed_wal(
                    fb,
                    multiplex_id,
                    queue_db,
                    inner_blobstores_scuba_table,
                    multiplex_scuba_table,
                    scuba_sample_rate,
                    blobstores,
                    write_quorum,
                    mysql_options,
                    readonly_storage,
                    blobstore_options,
                    logger,
                    config_store,
                    scrub_handler,
                    component_sampler,
                )
                .watched(logger)
                .await?
            }
            Logging {
                blobconfig,
                scuba_table,
                scuba_sample_rate,
            } => {
                needs_wrappers = false;
                let store = make_blobstore_put_ops(
                    fb,
                    *blobconfig,
                    mysql_options,
                    readonly_storage,
                    blobstore_options,
                    logger,
                    config_store,
                    scrub_handler,
                    component_sampler,
                    None,
                )
                .watched(logger)
                .await?;

                let scuba = scuba_table
                    .map_or(Ok(MononokeScubaSampleBuilder::with_discard()), |table| {
                        MononokeScubaSampleBuilder::new(fb, &table)
                    })?;
                Arc::new(LogBlob::new(store, scuba, scuba_sample_rate)) as Arc<dyn BlobstorePutOps>
            }
            Pack { .. } => {
                // NB packblob does not apply the wrappers internally
                make_packblob(
                    fb,
                    blobconfig,
                    readonly_storage,
                    blobstore_options,
                    logger,
                    config_store,
                )
                .watched(logger)
                .await
                .map(|store| Arc::new(store) as Arc<dyn BlobstorePutOps>)?
            }
        };

        let store = if needs_wrappers {
            let store = if let Some(component_sampler) = component_sampler {
                Arc::new(SamplingBlobstorePutOps::new(
                    store,
                    blobstore_id,
                    component_sampler.clone(),
                )) as Arc<dyn BlobstorePutOps>
            } else {
                store
            };

            let store = if readonly_storage.0 {
                Arc::new(ReadOnlyBlobstore::new(store)) as Arc<dyn BlobstorePutOps>
            } else {
                store
            };

            let store = if blobstore_options.throttle_options.has_throttle() {
                Arc::new(
                    ThrottledBlob::new(store, blobstore_options.throttle_options)
                        .watched(logger)
                        .await,
                ) as Arc<dyn BlobstorePutOps>
            } else {
                store
            };

            let store = if blobstore_options.chaos_options.has_chaos() {
                Arc::new(ChaosBlobstore::new(store, blobstore_options.chaos_options))
                    as Arc<dyn BlobstorePutOps>
            } else {
                store
            };

            if blobstore_options.delay_options.has_delay() {
                Arc::new(DelayedBlobstore::from_options(
                    store,
                    blobstore_options.delay_options,
                )) as Arc<dyn BlobstorePutOps>
            } else {
                store
            }
        } else {
            // Already applied the wrappers inside the store
            store
        };

        // NOTE: Do not add wrappers here that should only be added once per repository, since this
        // function will get called recursively for each member of a Multiplex! For those, use
        // RepoBlobstore::new instead.

        Ok(store)
    }
    .boxed()
}

async fn make_blobstore_multiplexed<'a>(
    fb: FacebookInit,
    multiplex_id: MultiplexId,
    queue_db: DatabaseConfig,
    scuba_table: Option<String>,
    multiplex_scuba_table: Option<String>,
    scuba_sample_rate: NonZeroU64,
    inner_config: Vec<(BlobstoreId, MultiplexedStoreType, BlobConfig)>,
    minimum_successful_writes: NonZeroUsize,
    not_present_read_quorum: NonZeroUsize,
    mysql_options: &'a MysqlOptions,
    readonly_storage: ReadOnlyStorage,
    blobstore_options: &'a BlobstoreOptions,
    logger: &'a Logger,
    config_store: &'a ConfigStore,
    scrub_handler: &'a Arc<dyn ScrubHandler>,
    component_sampler: Option<&'a Arc<dyn ComponentSamplingHandler>>,
) -> Result<Arc<dyn BlobstorePutOps>, Error> {
    let (normal_components, write_only_components) = setup_inner_blobstores(
        fb,
        inner_config,
        mysql_options,
        blobstore_options,
        logger,
        config_store,
        scrub_handler,
        component_sampler,
    )
    .await?;

    let queue = SqlBlobstoreSyncQueue::with_database_config(
        fb,
        &queue_db,
        mysql_options,
        readonly_storage.0,
    )?;

    let blobstore = match &blobstore_options.scrub_options {
        Some(scrub_options) => Arc::new(ScrubBlobstore::new(
            multiplex_id,
            normal_components,
            write_only_components,
            minimum_successful_writes,
            not_present_read_quorum,
            Arc::new(queue),
            scuba_table.map_or(Ok(MononokeScubaSampleBuilder::with_discard()), |table| {
                MononokeScubaSampleBuilder::new(fb, &table)
            })?,
            multiplex_scuba_table
                .map_or(Ok(MononokeScubaSampleBuilder::with_discard()), |table| {
                    MononokeScubaSampleBuilder::new(fb, &table)
                })?,
            scuba_sample_rate,
            scrub_options.clone(),
            scrub_handler.clone(),
        )) as Arc<dyn BlobstorePutOps>,
        None => Arc::new(MultiplexedBlobstore::new(
            multiplex_id,
            normal_components,
            write_only_components,
            minimum_successful_writes,
            not_present_read_quorum,
            Arc::new(queue),
            scuba_table.map_or(Ok(MononokeScubaSampleBuilder::with_discard()), |table| {
                MononokeScubaSampleBuilder::new(fb, &table)
            })?,
            multiplex_scuba_table
                .map_or(Ok(MononokeScubaSampleBuilder::with_discard()), |table| {
                    MononokeScubaSampleBuilder::new(fb, &table)
                })?,
            scuba_sample_rate,
        )) as Arc<dyn BlobstorePutOps>,
    };

    Ok(blobstore)
}

async fn make_multiplexed_wal<'a>(
    fb: FacebookInit,
    multiplex_id: MultiplexId,
    queue_db: ShardedDatabaseConfig,
    inner_blobstores_scuba_table: Option<String>,
    multiplex_scuba_table: Option<String>,
    scuba_sample_rate: NonZeroU64,
    inner_config: Vec<(BlobstoreId, MultiplexedStoreType, BlobConfig)>,
    write_quorum: usize,
    mysql_options: &'a MysqlOptions,
    readonly_storage: ReadOnlyStorage,
    blobstore_options: &'a BlobstoreOptions,
    logger: &'a Logger,
    config_store: &'a ConfigStore,
    scrub_handler: &'a Arc<dyn ScrubHandler>,
    component_sampler: Option<&'a Arc<dyn ComponentSamplingHandler>>,
) -> Result<Arc<dyn BlobstorePutOps>, Error> {
    let (normal_components, write_only_components) = setup_inner_blobstores(
        fb,
        inner_config,
        mysql_options,
        blobstore_options,
        logger,
        config_store,
        scrub_handler,
        component_sampler,
    )
    .await?;

    let wal_queue = Arc::new(SqlBlobstoreWal::with_sharded_database_config(
        fb,
        &queue_db,
        mysql_options,
        readonly_storage.0,
    )?);
    let scuba = WalScuba::new_from_raw(
        fb,
        inner_blobstores_scuba_table,
        multiplex_scuba_table,
        scuba_sample_rate,
    )?;

    let blobstore = match &blobstore_options.scrub_options {
        Some(scrub_options) => {
            Arc::new(WalScrubBlobstore::new(
                multiplex_id,
                wal_queue,
                normal_components,
                write_only_components,
                write_quorum,
                None, // use default timeouts
                scuba,
                scrub_options.clone(),
                scrub_handler.clone(),
            )?) as Arc<dyn BlobstorePutOps>
        }
        None => Arc::new(WalMultiplexedBlobstore::new(
            multiplex_id,
            wal_queue,
            normal_components,
            write_only_components,
            write_quorum,
            None, // use default timeouts
            scuba,
        )?) as Arc<dyn BlobstorePutOps>,
    };

    Ok(blobstore)
}

type InnerBlobstore = (BlobstoreId, Arc<dyn BlobstorePutOps>);

async fn setup_inner_blobstores<'a>(
    fb: FacebookInit,
    inner_config: Vec<(BlobstoreId, MultiplexedStoreType, BlobConfig)>,
    mysql_options: &'a MysqlOptions,
    blobstore_options: &'a BlobstoreOptions,
    logger: &'a Logger,
    config_store: &'a ConfigStore,
    scrub_handler: &'a Arc<dyn ScrubHandler>,
    component_sampler: Option<&'a Arc<dyn ComponentSamplingHandler>>,
) -> Result<(Vec<InnerBlobstore>, Vec<InnerBlobstore>), Error> {
    let component_readonly = blobstore_options
        .scrub_options
        .as_ref()
        .map_or(ReadOnlyStorage(false), |v| {
            ReadOnlyStorage(v.scrub_action != ScrubAction::Repair)
        });

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
                    scrub_handler,
                    component_sampler,
                    Some(blobstoreid),
                )
                .watched(logger)
                .await?;

                Result::<_, Error>::Ok((blobstoreid, store_type, store))
            }
        }
    }))
    .await?;

    // For now, `partition` could do this, but this will be easier to extend when we introduce more store types
    let mut normal_components = vec![];
    let mut write_only_components = vec![];
    for (blobstore_id, store_type, store) in components.into_iter() {
        match store_type {
            MultiplexedStoreType::Normal => normal_components.push((blobstore_id, store)),
            MultiplexedStoreType::WriteOnly => write_only_components.push((blobstore_id, store)),
        }
    }
    Ok((normal_components, write_only_components))
}
