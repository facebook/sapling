/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Repository factory.

use std::collections::HashMap;
use std::future::Future;
use std::hash::Hash;
use std::num::NonZeroUsize;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_once_cell::AsyncOnceCell;
use blobstore::Blobstore;
use blobstore_factory::{make_blobstore, make_metadata_sql_factory, MetadataSqlFactory};
use bonsai_git_mapping::{ArcBonsaiGitMapping, SqlBonsaiGitMappingConnection};
use bonsai_globalrev_mapping::{
    ArcBonsaiGlobalrevMapping, CachingBonsaiGlobalrevMapping, SqlBonsaiGlobalrevMapping,
};
use bonsai_hg_mapping::{ArcBonsaiHgMapping, CachingBonsaiHgMapping, SqlBonsaiHgMappingBuilder};
use bonsai_svnrev_mapping::{
    ArcRepoBonsaiSvnrevMapping, BonsaiSvnrevMapping, CachingBonsaiSvnrevMapping,
    RepoBonsaiSvnrevMapping, SqlBonsaiSvnrevMapping,
};
use bookmarks::{ArcBookmarkUpdateLog, ArcBookmarks};
use cacheblob::{
    new_cachelib_blobstore_no_lease, new_memcache_blobstore, CachelibBlobstoreOptions,
    InProcessLease, LeaseOps, MemcacheOps,
};
use cached_config::ConfigStore;
use changeset_fetcher::{ArcChangesetFetcher, SimpleChangesetFetcher};
use changesets::{ArcChangesets, CachingChangesets, SqlChangesets};
use context::CoreContext;
use dbbookmarks::{ArcSqlBookmarks, SqlBookmarksBuilder};
use fbinit::FacebookInit;
use filenodes::ArcFilenodes;
use filestore::{ArcFilestoreConfig, FilestoreConfig};
use futures_watchdog::WatchdogExt;
use mercurial_mutation::{ArcHgMutationStore, SqlHgMutationStoreBuilder};
use metaconfig_types::{
    ArcRepoConfig, BlobConfig, CensoredScubaParams, MetadataDatabaseConfig, Redaction, RepoConfig,
};
use newfilenodes::NewFilenodesBuilder;
use parking_lot::Mutex;
use phases::{ArcSqlPhasesFactory, SqlPhasesFactory};
use readonlyblob::ReadOnlyBlobstore;
use redactedblobstore::{RedactedMetadata, SqlRedactedContentStore};
use repo_blobstore::{ArcRepoBlobstore, RepoBlobstoreArgs};
use repo_derived_data::{ArcRepoDerivedData, RepoDerivedData};
use repo_identity::{ArcRepoIdentity, RepoIdentity};
use scuba_ext::MononokeScubaSampleBuilder;
use segmented_changelog::{new_server_segmented_changelog, SegmentedChangelogSqlConnections};
use segmented_changelog_types::ArcSegmentedChangelog;
use slog::Logger;
use sql_ext::facebook::MysqlOptions;
use thiserror::Error;
use virtually_sharded_blobstore::VirtuallyShardedBlobstore;

pub use blobstore_factory::{BlobstoreOptions, ReadOnlyStorage};

#[derive(Copy, Clone, PartialEq)]
pub enum Caching {
    /// Caching is enabled with the given number of shards.
    Enabled(usize),

    /// Caching is enabled only for the blobstore via cachelib, with the given
    /// number of shards.
    CachelibOnlyBlobstore(usize),

    /// Caching is not enabled.
    Disabled,
}

struct RepoFactoryCache<K: Clone + Eq + Hash, V: Clone> {
    cache: Mutex<HashMap<K, Arc<AsyncOnceCell<V>>>>,
}

impl<K: Clone + Eq + Hash, V: Clone> RepoFactoryCache<K, V> {
    fn new() -> Self {
        RepoFactoryCache {
            cache: Mutex::new(HashMap::new()),
        }
    }

    async fn get_or_try_init<F, Fut>(&self, key: &K, init: F) -> Result<V>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<V>>,
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

pub struct RepoFactory {
    fb: FacebookInit,
    logger: Logger,
    config_store: ConfigStore,
    mysql_options: MysqlOptions,
    blobstore_options: BlobstoreOptions,
    readonly_storage: ReadOnlyStorage,
    caching: Caching,
    censored_scuba_params: CensoredScubaParams,
    sql_factories: RepoFactoryCache<MetadataDatabaseConfig, Arc<MetadataSqlFactory>>,
    blobstores: RepoFactoryCache<BlobConfig, Arc<dyn Blobstore>>,
    redacted_blobs:
        RepoFactoryCache<MetadataDatabaseConfig, Arc<HashMap<String, RedactedMetadata>>>,
}

impl RepoFactory {
    pub fn new(
        fb: FacebookInit,
        logger: Logger,
        config_store: ConfigStore,
        mysql_options: MysqlOptions,
        blobstore_options: BlobstoreOptions,
        readonly_storage: ReadOnlyStorage,
        caching: Caching,
        censored_scuba_params: CensoredScubaParams,
    ) -> RepoFactory {
        RepoFactory {
            fb,
            logger,
            config_store,
            mysql_options,
            blobstore_options,
            readonly_storage,
            caching,
            censored_scuba_params,
            sql_factories: RepoFactoryCache::new(),
            blobstores: RepoFactoryCache::new(),
            redacted_blobs: RepoFactoryCache::new(),
        }
    }


    pub async fn sql_factory(
        &self,
        config: &MetadataDatabaseConfig,
    ) -> Result<Arc<MetadataSqlFactory>> {
        self.sql_factories
            .get_or_try_init(config, || async move {
                let sql_factory = make_metadata_sql_factory(
                    self.fb,
                    config.clone(),
                    self.mysql_options.clone(),
                    self.readonly_storage,
                    &self.logger,
                )
                .watched(&self.logger)
                .await?;
                Ok(Arc::new(sql_factory))
            })
            .await
    }

    async fn blobstore(&self, config: &BlobConfig) -> Result<Arc<dyn Blobstore>> {
        self.blobstores
            .get_or_try_init(config, || async move {
                let mut blobstore = make_blobstore(
                    self.fb,
                    config.clone(),
                    &self.mysql_options,
                    self.readonly_storage,
                    &self.blobstore_options,
                    &self.logger,
                    &self.config_store,
                )
                .watched(&self.logger)
                .await?;

                match self.caching {
                    Caching::Enabled(cache_shards) => {
                        let fb = self.fb;
                        let memcache_blobstore = tokio::task::spawn_blocking(move || {
                            new_memcache_blobstore(fb, blobstore, "multiplexed", "")
                        })
                        .await??;
                        blobstore = cachelib_blobstore(
                            memcache_blobstore,
                            cache_shards,
                            &self.blobstore_options.cachelib_options,
                        )?
                    }
                    Caching::CachelibOnlyBlobstore(cache_shards) => {
                        blobstore = cachelib_blobstore(
                            blobstore,
                            cache_shards,
                            &self.blobstore_options.cachelib_options,
                        )?;
                    }
                    Caching::Disabled => {}
                };

                Ok(blobstore)
            })
            .await
    }

    async fn redacted_blobs(
        &self,
        config: &MetadataDatabaseConfig,
    ) -> Result<Arc<HashMap<String, RedactedMetadata>>> {
        self.redacted_blobs
            .get_or_try_init(config, || async move {
                let sql_factory = self.sql_factory(config).await?;
                let redacted_content_store = sql_factory.open::<SqlRedactedContentStore>().await?;
                // Fetch redacted blobs in a separate task so that slow polls
                // in repo construction don't interfere with the SQL query.
                let redacted_blobs = tokio::task::spawn(async move {
                    redacted_content_store.get_all_redacted_blobs().await
                })
                .await??;
                Ok(Arc::new(redacted_blobs))
            })
            .await
    }

    /// Returns a named volatile pool if caching is enabled.
    fn maybe_volatile_pool(&self, name: &str) -> Result<Option<cachelib::VolatileLruCachePool>> {
        match self.caching {
            Caching::Enabled(_) => Ok(Some(volatile_pool(name)?)),
            _ => Ok(None),
        }
    }

    fn censored_scuba_builder(&self) -> Result<MononokeScubaSampleBuilder> {
        let mut builder = MononokeScubaSampleBuilder::with_opt_table(
            self.fb,
            self.censored_scuba_params.table.clone(),
        );
        builder.add_common_server_data();
        if let Some(scuba_log_file) = &self.censored_scuba_params.local_path {
            builder = builder.with_log_file(scuba_log_file)?;
        }
        Ok(builder)
    }
}

fn cache_pool(name: &str) -> Result<cachelib::LruCachePool> {
    Ok(cachelib::get_pool(name)
        .ok_or_else(|| RepoFactoryError::MissingCachePool(name.to_string()))?)
}

fn volatile_pool(name: &str) -> Result<cachelib::VolatileLruCachePool> {
    Ok(cachelib::get_volatile_pool(name)?
        .ok_or_else(|| RepoFactoryError::MissingCachePool(name.to_string()))?)
}

pub fn cachelib_blobstore<B: Blobstore + 'static>(
    blobstore: B,
    cache_shards: usize,
    options: &CachelibBlobstoreOptions,
) -> Result<Arc<dyn Blobstore>> {
    const BLOBSTORE_BLOBS_CACHE_POOL: &str = "blobstore-blobs";
    const BLOBSTORE_PRESENCE_CACHE_POOL: &str = "blobstore-presence";

    let blobstore: Arc<dyn Blobstore> = match NonZeroUsize::new(cache_shards) {
        Some(cache_shards) => {
            let blob_pool = volatile_pool(BLOBSTORE_BLOBS_CACHE_POOL)?;
            let presence_pool = volatile_pool(BLOBSTORE_PRESENCE_CACHE_POOL)?;

            Arc::new(VirtuallyShardedBlobstore::new(
                blobstore,
                blob_pool,
                presence_pool,
                cache_shards,
                options.clone(),
            ))
        }
        None => {
            let blob_pool = cache_pool(BLOBSTORE_BLOBS_CACHE_POOL)?;
            let presence_pool = cache_pool(BLOBSTORE_PRESENCE_CACHE_POOL)?;

            Arc::new(new_cachelib_blobstore_no_lease(
                blobstore,
                Arc::new(blob_pool),
                Arc::new(presence_pool),
                options.clone(),
            ))
        }
    };

    Ok(blobstore)
}

#[derive(Debug, Error)]
pub enum RepoFactoryError {
    #[error("Error opening changesets")]
    Changesets,

    #[error("Error opening bookmarks")]
    Bookmarks,

    #[error("Error opening phases")]
    Phases,

    #[error("Error opening bonsai-hg mapping")]
    BonsaiHgMapping,

    #[error("Error opening bonsai-git mapping")]
    BonsaiGitMapping,

    #[error("Error opening bonsai-globalrev mapping")]
    BonsaiGlobalrevMapping,

    #[error("Error opening bonsai-svnrev mapping")]
    BonsaiSvnrevMapping,

    #[error("Error opening filenodes")]
    Filenodes,

    #[error("Error opening hg mutation store")]
    HgMutationStore,

    #[error("Error opening segmented changelog")]
    SegmentedChangelog,

    #[error("Missing cache pool: {0}")]
    MissingCachePool(String),
}

#[facet::factory(name: String, config: RepoConfig)]
impl RepoFactory {
    pub fn repo_config(&self, config: &RepoConfig) -> ArcRepoConfig {
        Arc::new(config.clone())
    }

    pub fn repo_identity(&self, name: &str, repo_config: &ArcRepoConfig) -> ArcRepoIdentity {
        Arc::new(RepoIdentity::new(repo_config.repoid, name.to_string()))
    }

    pub async fn changesets(&self, repo_config: &ArcRepoConfig) -> Result<ArcChangesets> {
        let sql_factory = self
            .sql_factory(&repo_config.storage_config.metadata)
            .await?;
        let changesets = sql_factory
            .open::<SqlChangesets>()
            .await
            .context(RepoFactoryError::Changesets)?;
        if let Some(pool) = self.maybe_volatile_pool("changesets")? {
            Ok(Arc::new(CachingChangesets::new(
                self.fb,
                Arc::new(changesets),
                pool,
            )))
        } else {
            Ok(Arc::new(changesets))
        }
    }

    pub fn changeset_fetcher(
        &self,
        repo_identity: &ArcRepoIdentity,
        changesets: &ArcChangesets,
    ) -> ArcChangesetFetcher {
        Arc::new(SimpleChangesetFetcher::new(
            changesets.clone(),
            repo_identity.id(),
        ))
    }

    pub async fn sql_bookmarks(
        &self,
        repo_config: &ArcRepoConfig,
        repo_identity: &ArcRepoIdentity,
    ) -> Result<ArcSqlBookmarks> {
        let sql_factory = self
            .sql_factory(&repo_config.storage_config.metadata)
            .await?;
        let sql_bookmarks = sql_factory
            .open::<SqlBookmarksBuilder>()
            .await
            .context(RepoFactoryError::Bookmarks)?
            .with_repo_id(repo_identity.id());
        Ok(Arc::new(sql_bookmarks))
    }

    pub fn bookmarks(&self, sql_bookmarks: &ArcSqlBookmarks) -> ArcBookmarks {
        sql_bookmarks.clone()
    }

    pub fn bookmark_update_log(&self, sql_bookmarks: &ArcSqlBookmarks) -> ArcBookmarkUpdateLog {
        sql_bookmarks.clone()
    }

    pub async fn sql_phases_factory(
        &self,
        repo_config: &ArcRepoConfig,
    ) -> Result<ArcSqlPhasesFactory> {
        let sql_factory = self
            .sql_factory(&repo_config.storage_config.metadata)
            .await?;
        let mut sql_phases_factory = sql_factory
            .open::<SqlPhasesFactory>()
            .await
            .context(RepoFactoryError::Phases)?;
        if let Some(pool) = self.maybe_volatile_pool("phases")? {
            sql_phases_factory.enable_caching(self.fb, pool);
        }
        Ok(Arc::new(sql_phases_factory))
    }

    pub async fn bonsai_hg_mapping(
        &self,
        repo_config: &ArcRepoConfig,
    ) -> Result<ArcBonsaiHgMapping> {
        let sql_factory = self
            .sql_factory(&repo_config.storage_config.metadata)
            .await?;
        let builder = sql_factory
            .open::<SqlBonsaiHgMappingBuilder>()
            .await
            .context(RepoFactoryError::BonsaiHgMapping)?;
        let bonsai_hg_mapping = builder.build();
        if let Some(pool) = self.maybe_volatile_pool("bonsai_hg_mapping")? {
            Ok(Arc::new(CachingBonsaiHgMapping::new(
                self.fb,
                Arc::new(bonsai_hg_mapping),
                pool,
            )))
        } else {
            Ok(Arc::new(bonsai_hg_mapping))
        }
    }

    pub async fn bonsai_git_mapping(
        &self,
        repo_config: &ArcRepoConfig,
        repo_identity: &ArcRepoIdentity,
    ) -> Result<ArcBonsaiGitMapping> {
        let sql_factory = self
            .sql_factory(&repo_config.storage_config.metadata)
            .await?;
        let bonsai_git_mapping = sql_factory
            .open::<SqlBonsaiGitMappingConnection>()
            .await
            .context(RepoFactoryError::BonsaiGitMapping)?
            .with_repo_id(repo_identity.id());
        Ok(Arc::new(bonsai_git_mapping))
    }

    pub async fn bonsai_globalrev_mapping(
        &self,
        repo_config: &ArcRepoConfig,
    ) -> Result<ArcBonsaiGlobalrevMapping> {
        let sql_factory = self
            .sql_factory(&repo_config.storage_config.metadata)
            .await?;
        let bonsai_globalrev_mapping = sql_factory
            .open::<SqlBonsaiGlobalrevMapping>()
            .await
            .context(RepoFactoryError::BonsaiGlobalrevMapping)?;
        if let Some(pool) = self.maybe_volatile_pool("bonsai_globalrev_mapping")? {
            Ok(Arc::new(CachingBonsaiGlobalrevMapping::new(
                self.fb,
                Arc::new(bonsai_globalrev_mapping),
                pool,
            )))
        } else {
            Ok(Arc::new(bonsai_globalrev_mapping))
        }
    }

    pub async fn repo_bonsai_svnrev_mapping(
        &self,
        repo_config: &ArcRepoConfig,
        repo_identity: &ArcRepoIdentity,
    ) -> Result<ArcRepoBonsaiSvnrevMapping> {
        let sql_factory = self
            .sql_factory(&repo_config.storage_config.metadata)
            .await?;
        let bonsai_svnrev_mapping = sql_factory
            .open::<SqlBonsaiSvnrevMapping>()
            .await
            .context(RepoFactoryError::BonsaiSvnrevMapping)?;
        let bonsai_svnrev_mapping: Arc<dyn BonsaiSvnrevMapping + Send + Sync> =
            if let Some(pool) = self.maybe_volatile_pool("bonsai_svnrev_mapping")? {
                Arc::new(CachingBonsaiSvnrevMapping::new(
                    self.fb,
                    Arc::new(bonsai_svnrev_mapping),
                    pool,
                ))
            } else {
                Arc::new(bonsai_svnrev_mapping)
            };
        Ok(Arc::new(RepoBonsaiSvnrevMapping::new(
            repo_identity.id(),
            bonsai_svnrev_mapping,
        )))
    }

    pub async fn filenodes(&self, repo_config: &ArcRepoConfig) -> Result<ArcFilenodes> {
        let sql_factory = self
            .sql_factory(&repo_config.storage_config.metadata)
            .await?;
        let mut filenodes_builder = sql_factory
            .open_shardable::<NewFilenodesBuilder>()
            .await
            .context(RepoFactoryError::Filenodes)?;
        if let Caching::Enabled(_) = self.caching {
            let filenodes_tier = sql_factory.tier_info_shardable::<NewFilenodesBuilder>()?;
            let filenodes_pool = self
                .maybe_volatile_pool("filenodes")?
                .ok_or(RepoFactoryError::Filenodes)?;
            let filenodes_history_pool = self
                .maybe_volatile_pool("filenodes_history")?
                .ok_or(RepoFactoryError::Filenodes)?;
            filenodes_builder.enable_caching(
                self.fb,
                filenodes_pool,
                filenodes_history_pool,
                "newfilenodes",
                &filenodes_tier.tier_name,
            );
        }
        Ok(Arc::new(filenodes_builder.build()))
    }

    pub async fn hg_mutation_store(
        &self,
        repo_config: &ArcRepoConfig,
        repo_identity: &ArcRepoIdentity,
    ) -> Result<ArcHgMutationStore> {
        let sql_factory = self
            .sql_factory(&repo_config.storage_config.metadata)
            .await?;
        let hg_mutation_store = sql_factory
            .open::<SqlHgMutationStoreBuilder>()
            .await
            .context(RepoFactoryError::HgMutationStore)?
            .with_repo_id(repo_identity.id());
        Ok(Arc::new(hg_mutation_store))
    }

    pub async fn segmented_changelog(
        &self,
        repo_config: &ArcRepoConfig,
        repo_identity: &ArcRepoIdentity,
        changeset_fetcher: &ArcChangesetFetcher,
        bookmarks: &ArcBookmarks,
        repo_blobstore: &ArcRepoBlobstore,
    ) -> Result<ArcSegmentedChangelog> {
        let sql_factory = self
            .sql_factory(&repo_config.storage_config.metadata)
            .await?;
        let sql_connections = sql_factory
            .open::<SegmentedChangelogSqlConnections>()
            .await
            .context(RepoFactoryError::SegmentedChangelog)?;
        let pool = self.maybe_volatile_pool("segmented_changelog")?;
        let segmented_changelog = new_server_segmented_changelog(
            self.fb,
            &CoreContext::new_with_logger(self.fb, self.logger.clone()),
            repo_identity.id(),
            repo_config.segmented_changelog_config.clone(),
            sql_connections,
            changeset_fetcher.clone(),
            bookmarks.clone(),
            repo_blobstore.clone(),
            pool,
        )
        .await
        .context(RepoFactoryError::SegmentedChangelog)?;
        Ok(Arc::new(segmented_changelog))
    }

    pub fn repo_derived_data(&self, repo_config: &ArcRepoConfig) -> Result<ArcRepoDerivedData> {
        let config = repo_config.derived_data_config.clone();
        // Derived data leasing is performed through the cache, so is only
        // available if caching is enabled.
        let lease: Arc<dyn LeaseOps> = if let Caching::Enabled(_) = self.caching {
            Arc::new(MemcacheOps::new(self.fb, "derived-data-lease", "")?)
        } else {
            Arc::new(InProcessLease::new())
        };
        Ok(Arc::new(RepoDerivedData::new(config, lease)))
    }

    pub async fn repo_blobstore(
        &self,
        repo_identity: &ArcRepoIdentity,
        repo_config: &ArcRepoConfig,
    ) -> Result<ArcRepoBlobstore> {
        let mut blobstore = self
            .blobstore(&repo_config.storage_config.blobstore)
            .await?;

        if self.readonly_storage.0 {
            blobstore = Arc::new(ReadOnlyBlobstore::new(blobstore));
        }

        let redacted_blobs = match repo_config.redaction {
            Redaction::Enabled => {
                let redacted_blobs = self
                    .redacted_blobs(&repo_config.storage_config.metadata)
                    .await?;
                // TODO: Make RepoBlobstore take Arc<...> so it can share the hashmap.
                Some(redacted_blobs.as_ref().clone())
            }
            Redaction::Disabled => None,
        };

        let censored_scuba_builder = self.censored_scuba_builder()?;

        let repo_blobstore_args = RepoBlobstoreArgs::new(
            blobstore,
            redacted_blobs,
            repo_identity.id(),
            censored_scuba_builder,
        );
        let (repo_blobstore, _repo_id) = repo_blobstore_args.into_blobrepo_parts();

        Ok(Arc::new(repo_blobstore))
    }

    pub fn filestore_config(&self, repo_config: &ArcRepoConfig) -> ArcFilestoreConfig {
        let filestore_config = repo_config
            .filestore
            .as_ref()
            .map(|p| FilestoreConfig {
                chunk_size: Some(p.chunk_size),
                concurrency: p.concurrency,
            })
            .unwrap_or_default();
        Arc::new(filestore_config)
    }
}
