/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Repository factory.
#![feature(trait_alias)]

use std::collections::HashMap;
use std::future::Future;
use std::hash::Hash;
use std::num::NonZeroUsize;
use std::sync::Arc;

use acl_regions::build_acl_regions;
use acl_regions::ArcAclRegions;
use anyhow::Context;
use anyhow::Result;
use async_once_cell::AsyncOnceCell;
use blobstore::Blobstore;
use blobstore::BlobstoreEnumerableWithUnlink;
use blobstore_factory::default_scrub_handler;
use blobstore_factory::make_blobstore;
use blobstore_factory::make_blobstore_enumerable_with_unlink;
use blobstore_factory::make_metadata_sql_factory;
pub use blobstore_factory::BlobstoreOptions;
use blobstore_factory::ComponentSamplingHandler;
use blobstore_factory::MetadataSqlFactory;
pub use blobstore_factory::ReadOnlyStorage;
use blobstore_factory::ScrubHandler;
use bonsai_git_mapping::ArcBonsaiGitMapping;
use bonsai_git_mapping::SqlBonsaiGitMappingBuilder;
use bonsai_globalrev_mapping::ArcBonsaiGlobalrevMapping;
use bonsai_globalrev_mapping::CachingBonsaiGlobalrevMapping;
use bonsai_globalrev_mapping::SqlBonsaiGlobalrevMappingBuilder;
use bonsai_hg_mapping::ArcBonsaiHgMapping;
use bonsai_hg_mapping::CachingBonsaiHgMapping;
use bonsai_hg_mapping::SqlBonsaiHgMappingBuilder;
use bonsai_svnrev_mapping::ArcBonsaiSvnrevMapping;
use bonsai_svnrev_mapping::CachingBonsaiSvnrevMapping;
use bonsai_svnrev_mapping::SqlBonsaiSvnrevMappingBuilder;
use bookmarks::bookmark_heads_fetcher;
use bookmarks::ArcBookmarkUpdateLog;
use bookmarks::ArcBookmarks;
use bookmarks::CachedBookmarks;
use cacheblob::new_cachelib_blobstore_no_lease;
use cacheblob::new_memcache_blobstore;
use cacheblob::CachelibBlobstoreOptions;
use cacheblob::InProcessLease;
use cacheblob::LeaseOps;
use cacheblob::MemcacheOps;
use changeset_fetcher::ArcChangesetFetcher;
use changeset_fetcher::SimpleChangesetFetcher;
use changesets::ArcChangesets;
use changesets_impl::CachingChangesets;
use changesets_impl::SqlChangesetsBuilder;
use cloned::cloned;
use context::CoreContext;
use context::SessionContainer;
use cross_repo_sync::create_commit_syncer_lease;
use dbbookmarks::ArcSqlBookmarks;
use dbbookmarks::SqlBookmarksBuilder;
#[cfg(fbcode_build)]
use derived_data_client_library::Client as DerivationServiceClient;
use derived_data_remote::DerivationClient;
use derived_data_remote::RemoteDerivationOptions;
use environment::Caching;
use environment::MononokeEnvironment;
use environment::WarmBookmarksCacheDerivedData;
use ephemeral_blobstore::ArcRepoEphemeralStore;
use ephemeral_blobstore::RepoEphemeralStore;
use ephemeral_blobstore::RepoEphemeralStoreBuilder;
use fbinit::FacebookInit;
use filenodes::ArcFilenodes;
use filestore::ArcFilestoreConfig;
use filestore::FilestoreConfig;
use futures_watchdog::WatchdogExt;
use hooks::hook_loader::load_hooks;
use hooks::ArcHookManager;
use hooks::HookManager;
use hooks_content_stores::RepoFileContentManager;
use hooks_content_stores::TextOnlyFileContentManager;
use live_commit_sync_config::CfgrLiveCommitSyncConfig;
use memcache::KeyGen;
use memcache::MemcacheClient;
use mercurial_mutation::ArcHgMutationStore;
use mercurial_mutation::CachedHgMutationStore;
use mercurial_mutation::SqlHgMutationStoreBuilder;
use metaconfig_types::ArcCommonConfig;
use metaconfig_types::ArcRepoConfig;
use metaconfig_types::BlobConfig;
use metaconfig_types::CommonConfig;
use metaconfig_types::MetadataDatabaseConfig;
use metaconfig_types::Redaction;
use metaconfig_types::RepoConfig;
use metaconfig_types::RepoReadOnly;
use mutable_counters::ArcMutableCounters;
use mutable_counters::SqlMutableCountersBuilder;
use mutable_renames::ArcMutableRenames;
use mutable_renames::MutableRenames;
use mutable_renames::SqlMutableRenamesStore;
use newfilenodes::NewFilenodesBuilder;
use parking_lot::Mutex;
use permission_checker::AclProvider;
use phases::ArcPhases;
use pushrebase_mutation_mapping::ArcPushrebaseMutationMapping;
use pushrebase_mutation_mapping::SqlPushrebaseMutationMappingConnection;
use readonlyblob::ReadOnlyBlobstore;
use redactedblobstore::ArcRedactionConfigBlobstore;
use redactedblobstore::RedactedBlobs;
use redactedblobstore::RedactionConfigBlobstore;
use redactedblobstore::SqlRedactedContentStore;
use repo_blobstore::ArcRepoBlobstore;
use repo_blobstore::RepoBlobstore;
use repo_bookmark_attrs::ArcRepoBookmarkAttrs;
use repo_bookmark_attrs::RepoBookmarkAttrs;
use repo_cross_repo::ArcRepoCrossRepo;
use repo_cross_repo::RepoCrossRepo;
use repo_derived_data::ArcRepoDerivedData;
use repo_derived_data::RepoDerivedData;
use repo_derived_data_service::ArcDerivedDataManagerSet;
use repo_derived_data_service::DerivedDataManagerSet;
use repo_identity::ArcRepoIdentity;
use repo_identity::RepoIdentity;
use repo_lock::AlwaysLockedRepoLock;
use repo_lock::ArcRepoLock;
use repo_lock::MutableRepoLock;
use repo_lock::SqlRepoLock;
use repo_permission_checker::ArcRepoPermissionChecker;
use repo_permission_checker::ProdRepoPermissionChecker;
use repo_sparse_profiles::ArcRepoSparseProfiles;
use repo_sparse_profiles::RepoSparseProfiles;
use repo_sparse_profiles::SqlSparseProfilesSizes;
use requests_table::ArcLongRunningRequestsQueue;
use requests_table::SqlLongRunningRequestsQueue;
use scuba_ext::MononokeScubaSampleBuilder;
use segmented_changelog::new_server_segmented_changelog;
use segmented_changelog::new_server_segmented_changelog_manager;
use segmented_changelog::ArcSegmentedChangelogManager;
use segmented_changelog::SegmentedChangelogSqlConnections;
use segmented_changelog_types::ArcSegmentedChangelog;
use skiplist::ArcSkiplistIndex;
use skiplist::SkiplistIndex;
use slog::o;
use sql::SqlConnections;
use sql::SqlConnectionsWithSchema;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromDatabaseConfig;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_query_config::ArcSqlQueryConfig;
use sql_query_config::SqlQueryConfig;
use sqlphases::SqlPhasesBuilder;
use streaming_clone::ArcStreamingClone;
use streaming_clone::StreamingCloneBuilder;
use synced_commit_mapping::ArcSyncedCommitMapping;
use synced_commit_mapping::SqlSyncedCommitMapping;
use thiserror::Error;
use tunables::tunables;
use virtually_sharded_blobstore::VirtuallyShardedBlobstore;
use warm_bookmarks_cache::ArcBookmarksCache;
use warm_bookmarks_cache::NoopBookmarksCache;
use warm_bookmarks_cache::WarmBookmarksCacheBuilder;
use wireproto_handler::ArcPushRedirectorMode;
use wireproto_handler::ArcRepoHandlerBase;
use wireproto_handler::ArcTargetRepoDbs;
use wireproto_handler::PushRedirectorBase;
use wireproto_handler::PushRedirectorMode;
use wireproto_handler::PushRedirectorMode::Enabled;
use wireproto_handler::RepoHandlerBase;
use wireproto_handler::TargetRepoDbs;

const DERIVED_DATA_LEASE: &str = "derived-data-lease";

#[derive(Clone)]
struct RepoFactoryCache<K: Clone + Eq + Hash, V: Clone> {
    cache: Arc<Mutex<HashMap<K, Arc<AsyncOnceCell<V>>>>>,
}

impl<K: Clone + Eq + Hash, V: Clone> RepoFactoryCache<K, V> {
    fn new() -> Self {
        RepoFactoryCache {
            cache: Arc::new(Mutex::new(HashMap::new())),
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

pub trait RepoFactoryOverride<T> = Fn(T) -> T + Send + Sync + 'static;

#[derive(Clone)]
pub struct RepoFactory {
    pub env: Arc<MononokeEnvironment>,
    sql_factories: RepoFactoryCache<MetadataDatabaseConfig, Arc<MetadataSqlFactory>>,
    sql_connections: RepoFactoryCache<MetadataDatabaseConfig, SqlConnectionsWithSchema>,
    blobstores: RepoFactoryCache<BlobConfig, Arc<dyn Blobstore>>,
    redacted_blobs: RepoFactoryCache<MetadataDatabaseConfig, Arc<RedactedBlobs>>,
    blobstore_override: Option<Arc<dyn RepoFactoryOverride<Arc<dyn Blobstore>>>>,
    scrub_handler: Arc<dyn ScrubHandler>,
    blobstore_component_sampler: Option<Arc<dyn ComponentSamplingHandler>>,
    bonsai_hg_mapping_overwrite: bool,
}

impl RepoFactory {
    pub fn new(env: Arc<MononokeEnvironment>) -> RepoFactory {
        RepoFactory {
            sql_factories: RepoFactoryCache::new(),
            sql_connections: RepoFactoryCache::new(),
            blobstores: RepoFactoryCache::new(),
            redacted_blobs: RepoFactoryCache::new(),
            blobstore_override: None,
            scrub_handler: default_scrub_handler(),
            blobstore_component_sampler: None,
            bonsai_hg_mapping_overwrite: false,
            env,
        }
    }

    pub fn with_blobstore_override(
        &mut self,
        blobstore_override: impl RepoFactoryOverride<Arc<dyn Blobstore>>,
    ) -> &mut Self {
        self.blobstore_override = Some(Arc::new(blobstore_override));
        self
    }

    pub fn with_scrub_handler(&mut self, scrub_handler: Arc<dyn ScrubHandler>) -> &mut Self {
        self.scrub_handler = scrub_handler;
        self
    }

    pub fn with_blobstore_component_sampler(
        &mut self,
        handler: Arc<dyn ComponentSamplingHandler>,
    ) -> &mut Self {
        self.blobstore_component_sampler = Some(handler);
        self
    }

    pub fn with_bonsai_hg_mapping_override(&mut self) -> &mut Self {
        self.bonsai_hg_mapping_overwrite = true;
        self
    }

    pub async fn sql_factory(
        &self,
        config: &MetadataDatabaseConfig,
    ) -> Result<Arc<MetadataSqlFactory>> {
        self.sql_factories
            .get_or_try_init(config, || async move {
                let sql_factory = make_metadata_sql_factory(
                    self.env.fb,
                    config.clone(),
                    self.env.mysql_options.clone(),
                    self.env.readonly_storage,
                )
                .watched(&self.env.logger)
                .await?;
                Ok(Arc::new(sql_factory))
            })
            .await
    }

    async fn sql_connections(
        &self,
        config: &MetadataDatabaseConfig,
    ) -> Result<SqlConnectionsWithSchema> {
        self.sql_connections_with_label(config, "metadata").await
    }

    async fn sql_connections_with_label(
        &self,
        config: &MetadataDatabaseConfig,
        label: &str,
    ) -> Result<SqlConnectionsWithSchema> {
        let sql_factory = self.sql_factory(config).await?;
        self.sql_connections
            .get_or_try_init(config, || async move {
                sql_factory
                    .make_primary_connections(label.to_string())
                    .await
            })
            .await
    }

    async fn open<T: SqlConstruct>(&self, config: &MetadataDatabaseConfig) -> Result<T> {
        let sql_connections = match config {
            // For sqlite cache the connections to save reopening the file
            MetadataDatabaseConfig::Local(_) => self.sql_connections(config).await?,
            // TODO(ahornby) for other dbs the label can be part of connection identity in stats so don't reuse
            _ => {
                self.sql_factory(config)
                    .await?
                    .make_primary_connections(T::LABEL.to_string())
                    .await?
            }
        };
        T::from_connections_with_schema(sql_connections)
    }

    async fn blobstore_no_cache(&self, config: &BlobConfig) -> Result<Arc<dyn Blobstore>> {
        make_blobstore(
            self.env.fb,
            config.clone(),
            &self.env.mysql_options,
            self.env.readonly_storage,
            &self.env.blobstore_options,
            &self.env.logger,
            &self.env.config_store,
            &self.scrub_handler,
            self.blobstore_component_sampler.as_ref(),
        )
        .watched(&self.env.logger)
        .await
    }

    async fn repo_blobstore_from_blobstore(
        &self,
        repo_identity: &ArcRepoIdentity,
        repo_config: &ArcRepoConfig,
        blobstore: &Arc<dyn Blobstore>,
        common_config: &ArcCommonConfig,
    ) -> Result<RepoBlobstore> {
        let mut blobstore = blobstore.clone();
        if self.env.readonly_storage.0 {
            blobstore = Arc::new(ReadOnlyBlobstore::new(blobstore));
        }

        let redacted_blobs = match repo_config.redaction {
            Redaction::Enabled => {
                let redacted_blobs = self
                    .redacted_blobs(
                        self.ctx(None),
                        &repo_config.storage_config.metadata,
                        common_config,
                    )
                    .await?;
                Some(redacted_blobs)
            }
            Redaction::Disabled => None,
        };

        let censored_scuba_builder = self.censored_scuba_builder(common_config)?;

        let repo_blobstore = RepoBlobstore::new(
            blobstore,
            redacted_blobs,
            repo_identity.id(),
            censored_scuba_builder,
        );

        Ok(repo_blobstore)
    }

    async fn blobstore_enumerable_with_unlink(
        &self,
        config: &BlobConfig,
    ) -> Result<Arc<dyn BlobstoreEnumerableWithUnlink>> {
        make_blobstore_enumerable_with_unlink(
            self.env.fb,
            config.clone(),
            &self.env.blobstore_options,
            &self.env.logger,
        )
        .watched(&self.env.logger)
        .await
    }

    async fn blobstore(&self, config: &BlobConfig) -> Result<Arc<dyn Blobstore>> {
        self.blobstores
            .get_or_try_init(config, || async move {
                let mut blobstore = self.blobstore_no_cache(config).await?;

                match self.env.caching {
                    Caching::Enabled(cache_shards) => {
                        let fb = self.env.fb;
                        let memcache_blobstore = tokio::task::spawn_blocking(move || {
                            new_memcache_blobstore(fb, blobstore, "multiplexed", "")
                        })
                        .await??;
                        blobstore = cachelib_blobstore(
                            memcache_blobstore,
                            cache_shards,
                            &self.env.blobstore_options.cachelib_options,
                        )?
                    }
                    Caching::CachelibOnlyBlobstore(cache_shards) => {
                        blobstore = cachelib_blobstore(
                            blobstore,
                            cache_shards,
                            &self.env.blobstore_options.cachelib_options,
                        )?;
                    }
                    Caching::Disabled => {}
                };

                if let Some(blobstore_override) = &self.blobstore_override {
                    blobstore = blobstore_override(blobstore);
                }

                Ok(blobstore)
            })
            .await
    }

    pub async fn redacted_blobs(
        &self,
        ctx: CoreContext,
        db_config: &MetadataDatabaseConfig,
        common_config: &ArcCommonConfig,
    ) -> Result<Arc<RedactedBlobs>> {
        self.redacted_blobs
            .get_or_try_init(db_config, || async move {
                let redacted_blobs = if tunables().get_redaction_config_from_xdb() {
                    let redacted_content_store =
                        self.open::<SqlRedactedContentStore>(db_config).await?;
                    // Fetch redacted blobs in a separate task so that slow polls
                    // in repo construction don't interfere with the SQL query.
                    tokio::task::spawn(async move {
                        redacted_content_store.get_all_redacted_blobs().await
                    })
                    .await??
                } else {
                    let blobstore = self.redaction_config_blobstore(common_config).await?;
                    RedactedBlobs::from_configerator(
                        &self.env.config_store,
                        &common_config.redaction_config.redaction_sets_location,
                        ctx,
                        blobstore,
                    )
                    .await?
                };
                Ok(Arc::new(redacted_blobs))
            })
            .await
    }

    pub async fn redaction_config_blobstore_from_config(
        &self,
        config: &BlobConfig,
    ) -> Result<ArcRedactionConfigBlobstore> {
        let blobstore = self.blobstore(config).await?;
        Ok(Arc::new(RedactionConfigBlobstore::new(blobstore)))
    }

    fn ctx(&self, repo_identity: Option<&ArcRepoIdentity>) -> CoreContext {
        let logger = repo_identity.map_or_else(
            || self.env.logger.new(o!()),
            |id| {
                let repo_name = String::from(id.name());
                self.env.logger.new(o!("repo" => repo_name))
            },
        );
        let session = SessionContainer::new_with_defaults(self.env.fb);
        session.new_context(logger, self.env.scuba_sample_builder.clone())
    }

    /// Returns a named volatile pool if caching is enabled.
    fn maybe_volatile_pool(&self, name: &str) -> Result<Option<cachelib::VolatileLruCachePool>> {
        match self.env.caching {
            Caching::Enabled(_) => Ok(Some(volatile_pool(name)?)),
            _ => Ok(None),
        }
    }

    fn censored_scuba_builder(
        &self,
        config: &ArcCommonConfig,
    ) -> Result<MononokeScubaSampleBuilder> {
        let censored_scuba_params = &config.censored_scuba_params;
        let mut builder = MononokeScubaSampleBuilder::with_opt_table(
            self.env.fb,
            censored_scuba_params.table.clone(),
        )?;
        builder.add_common_server_data();
        if let Some(scuba_log_file) = &censored_scuba_params.local_path {
            builder = builder.with_log_file(scuba_log_file)?;
        }
        Ok(builder)
    }

    pub fn acl_provider(&self) -> &dyn AclProvider {
        self.env.acl_provider.as_ref()
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

    #[error("Error opening pushrebase mutation mapping")]
    PushrebaseMutationMapping,

    #[error("Error opening filenodes")]
    Filenodes,

    #[error("Error opening hg mutation store")]
    HgMutationStore,

    #[error("Error opening segmented changelog")]
    SegmentedChangelog,

    #[error("Error starting segmented changelog manager")]
    SegmentedChangelogManager,

    #[error("Missing cache pool: {0}")]
    MissingCachePool(String),

    #[error("Error opening long-running request queue")]
    LongRunningRequestsQueue,

    #[error("Error opening mutable renames")]
    MutableRenames,

    #[error("Error opening cross repo sync mapping")]
    RepoCrossRepo,

    #[error("Error opening mutable counters")]
    MutableCounters,

    #[error("Error creating hook manager")]
    HookManager,

    #[error("Error creating bookmark attributes")]
    RepoBookmarkAttrs,

    #[error("Error creating streaming clone")]
    StreamingClone,

    #[error("Error creating push redirector base")]
    PushRedirectorBase,

    #[error("Error creating target repo DB")]
    TargetRepoDbs,

    #[error("Error creating repo handler base")]
    RepoHandlerBase,
}

#[facet::factory(name: String, repo_config_param: RepoConfig, common_config_param: CommonConfig)]
impl RepoFactory {
    pub fn repo_config(&self, repo_config_param: &RepoConfig) -> ArcRepoConfig {
        Arc::new(repo_config_param.clone())
    }

    pub fn common_config(&self, common_config_param: &CommonConfig) -> ArcCommonConfig {
        Arc::new(common_config_param.clone())
    }

    pub fn repo_identity(&self, name: &str, repo_config: &ArcRepoConfig) -> ArcRepoIdentity {
        Arc::new(RepoIdentity::new(repo_config.repoid, name.to_string()))
    }

    pub fn caching(&self) -> Caching {
        self.env.caching
    }

    pub async fn changesets(
        &self,
        repo_identity: &ArcRepoIdentity,
        repo_config: &ArcRepoConfig,
    ) -> Result<ArcChangesets> {
        let builder = self
            .open::<SqlChangesetsBuilder>(&repo_config.storage_config.metadata)
            .await
            .context(RepoFactoryError::Changesets)?;
        let changesets = builder.build(self.env.rendezvous_options, repo_identity.id());
        if let Some(pool) = self.maybe_volatile_pool("changesets")? {
            Ok(Arc::new(CachingChangesets::new(
                self.env.fb,
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
        let sql_bookmarks = self
            .open::<SqlBookmarksBuilder>(&repo_config.storage_config.metadata)
            .await
            .context(RepoFactoryError::Bookmarks)?
            .with_repo_id(repo_identity.id());

        Ok(Arc::new(sql_bookmarks))
    }

    pub fn bookmarks(
        &self,
        sql_bookmarks: &ArcSqlBookmarks,
        repo_identity: &ArcRepoIdentity,
    ) -> ArcBookmarks {
        Arc::new(CachedBookmarks::new(
            sql_bookmarks.clone(),
            repo_identity.id(),
        ))
    }

    pub fn bookmark_update_log(&self, sql_bookmarks: &ArcSqlBookmarks) -> ArcBookmarkUpdateLog {
        sql_bookmarks.clone()
    }

    pub async fn phases(
        &self,
        repo_identity: &ArcRepoIdentity,
        repo_config: &ArcRepoConfig,
        bookmarks: &ArcBookmarks,
        changeset_fetcher: &ArcChangesetFetcher,
    ) -> Result<ArcPhases> {
        let mut sql_phases_builder = self
            .open::<SqlPhasesBuilder>(&repo_config.storage_config.metadata)
            .await
            .context(RepoFactoryError::Phases)?;
        if let Some(pool) = self.maybe_volatile_pool("phases")? {
            sql_phases_builder.enable_caching(self.env.fb, pool);
        }
        let heads_fetcher = bookmark_heads_fetcher(bookmarks.clone());
        Ok(sql_phases_builder.build(repo_identity.id(), changeset_fetcher.clone(), heads_fetcher))
    }

    pub async fn bonsai_hg_mapping(
        &self,
        repo_identity: &ArcRepoIdentity,
        repo_config: &ArcRepoConfig,
    ) -> Result<ArcBonsaiHgMapping> {
        let mut builder = self
            .open::<SqlBonsaiHgMappingBuilder>(&repo_config.storage_config.metadata)
            .await
            .context(RepoFactoryError::BonsaiHgMapping)?;

        if self.bonsai_hg_mapping_overwrite {
            builder = builder.with_overwrite();
        }

        let bonsai_hg_mapping = builder.build(repo_identity.id(), self.env.rendezvous_options);

        if let Some(pool) = self.maybe_volatile_pool("bonsai_hg_mapping")? {
            Ok(Arc::new(CachingBonsaiHgMapping::new(
                self.env.fb,
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
        let bonsai_git_mapping = self
            .open::<SqlBonsaiGitMappingBuilder>(&repo_config.storage_config.metadata)
            .await
            .context(RepoFactoryError::BonsaiGitMapping)?
            .build(repo_identity.id());
        Ok(Arc::new(bonsai_git_mapping))
    }

    pub async fn long_running_requests_queue(
        &self,
        repo_config: &ArcRepoConfig,
    ) -> Result<ArcLongRunningRequestsQueue> {
        let long_running_requests_queue = self
            .open::<SqlLongRunningRequestsQueue>(&repo_config.storage_config.metadata)
            .await
            .context(RepoFactoryError::LongRunningRequestsQueue)?;
        Ok(Arc::new(long_running_requests_queue))
    }

    pub async fn bonsai_globalrev_mapping(
        &self,
        repo_config: &ArcRepoConfig,
        repo_identity: &ArcRepoIdentity,
    ) -> Result<ArcBonsaiGlobalrevMapping> {
        let bonsai_globalrev_mapping = self
            .open::<SqlBonsaiGlobalrevMappingBuilder>(&repo_config.storage_config.metadata)
            .await
            .context(RepoFactoryError::BonsaiGlobalrevMapping)?
            .build(repo_identity.id());
        if let Some(pool) = self.maybe_volatile_pool("bonsai_globalrev_mapping")? {
            Ok(Arc::new(CachingBonsaiGlobalrevMapping::new(
                self.env.fb,
                Arc::new(bonsai_globalrev_mapping),
                pool,
            )))
        } else {
            Ok(Arc::new(bonsai_globalrev_mapping))
        }
    }

    pub async fn bonsai_svnrev_mapping(
        &self,
        repo_config: &ArcRepoConfig,
        repo_identity: &ArcRepoIdentity,
    ) -> Result<ArcBonsaiSvnrevMapping> {
        let bonsai_svnrev_mapping = self
            .open::<SqlBonsaiSvnrevMappingBuilder>(&repo_config.storage_config.metadata)
            .await
            .context(RepoFactoryError::BonsaiSvnrevMapping)?
            .build(repo_identity.id());
        if let Some(pool) = self.maybe_volatile_pool("bonsai_svnrev_mapping")? {
            Ok(Arc::new(CachingBonsaiSvnrevMapping::new(
                self.env.fb,
                Arc::new(bonsai_svnrev_mapping),
                pool,
            )))
        } else {
            Ok(Arc::new(bonsai_svnrev_mapping))
        }
    }

    pub async fn pushrebase_mutation_mapping(
        &self,
        repo_config: &ArcRepoConfig,
    ) -> Result<ArcPushrebaseMutationMapping> {
        let conn = self
            .open::<SqlPushrebaseMutationMappingConnection>(&repo_config.storage_config.metadata)
            .await
            .context(RepoFactoryError::PushrebaseMutationMapping)?;
        Ok(Arc::new(conn.with_repo_id(repo_config.repoid)))
    }

    pub async fn permission_checker(
        &self,
        repo_config: &ArcRepoConfig,
        repo_identity: &ArcRepoIdentity,
        common_config: &ArcCommonConfig,
    ) -> Result<ArcRepoPermissionChecker> {
        let repo_name = repo_identity.name();
        let permission_checker = ProdRepoPermissionChecker::new(
            &self.env.logger,
            self.env.acl_provider.as_ref(),
            repo_config.hipster_acl.as_deref(),
            repo_config
                .source_control_service
                .service_write_hipster_acl
                .as_deref(),
            repo_config
                .acl_region_config
                .as_ref()
                .map(|config| {
                    config
                        .allow_rules
                        .iter()
                        .map(|rule| rule.hipster_acl.as_str())
                        .collect()
                })
                .unwrap_or_default(),
            repo_name,
            &common_config.global_allowlist,
        )
        .await?;
        Ok(Arc::new(permission_checker))
    }

    pub async fn filenodes(
        &self,
        repo_config: &ArcRepoConfig,
        repo_identity: &ArcRepoIdentity,
    ) -> Result<ArcFilenodes> {
        let sql_factory = self
            .sql_factory(&repo_config.storage_config.metadata)
            .await?;
        let mut filenodes_builder = tokio::task::spawn_blocking({
            cloned!(sql_factory);
            move || {
                sql_factory
                    .open_shardable::<NewFilenodesBuilder>()
                    .context(RepoFactoryError::Filenodes)
            }
        })
        .await??;
        if let Caching::Enabled(_) = self.env.caching {
            let filenodes_tier = sql_factory.tier_info_shardable::<NewFilenodesBuilder>()?;
            let filenodes_pool = self
                .maybe_volatile_pool("filenodes")?
                .ok_or(RepoFactoryError::Filenodes)?;
            let filenodes_history_pool = self
                .maybe_volatile_pool("filenodes_history")?
                .ok_or(RepoFactoryError::Filenodes)?;
            filenodes_builder.enable_caching(
                self.env.fb,
                filenodes_pool,
                filenodes_history_pool,
                "newfilenodes",
                &filenodes_tier.tier_name,
            );
        }
        Ok(Arc::new(filenodes_builder.build(repo_identity.id())))
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
            .context(RepoFactoryError::HgMutationStore)?
            .with_repo_id(repo_identity.id());

        if let Some(pool) = self.maybe_volatile_pool("hg_mutation_store")? {
            Ok(Arc::new(CachedHgMutationStore::new(
                self.env.fb,
                Arc::new(hg_mutation_store),
                pool,
            )))
        } else {
            Ok(Arc::new(hg_mutation_store))
        }
    }

    pub async fn segmented_changelog(
        &self,
        repo_config: &ArcRepoConfig,
        repo_identity: &ArcRepoIdentity,
        changeset_fetcher: &ArcChangesetFetcher,
        bookmarks: &ArcBookmarks,
        repo_blobstore: &ArcRepoBlobstore,
    ) -> Result<ArcSegmentedChangelog> {
        let sql_connections = self
            .open::<SegmentedChangelogSqlConnections>(&repo_config.storage_config.metadata)
            .await
            .context(RepoFactoryError::SegmentedChangelog)?;
        let pool = self.maybe_volatile_pool("segmented_changelog")?;
        let segmented_changelog = new_server_segmented_changelog(
            self.env.fb,
            &self.ctx(Some(repo_identity)),
            repo_identity,
            repo_config.segmented_changelog_config.clone(),
            sql_connections,
            changeset_fetcher.clone(),
            bookmarks.clone(),
            repo_blobstore.clone(),
            pool,
        )
        .await
        .context(RepoFactoryError::SegmentedChangelog)?;
        Ok(segmented_changelog)
    }

    pub async fn segmented_changelog_manager(
        &self,
        repo_config: &ArcRepoConfig,
        repo_identity: &ArcRepoIdentity,
        changeset_fetcher: &ArcChangesetFetcher,
        bookmarks: &ArcBookmarks,
        repo_blobstore: &ArcRepoBlobstore,
    ) -> Result<ArcSegmentedChangelogManager> {
        let sql_connections = self
            .open::<SegmentedChangelogSqlConnections>(&repo_config.storage_config.metadata)
            .await
            .context(RepoFactoryError::SegmentedChangelog)?;
        let pool = self.maybe_volatile_pool("segmented_changelog")?;
        let manager = new_server_segmented_changelog_manager(
            self.env.fb,
            &self.ctx(Some(repo_identity)),
            repo_identity,
            repo_config.segmented_changelog_config.clone(),
            sql_connections,
            changeset_fetcher.clone(),
            bookmarks.clone(),
            repo_blobstore.clone(),
            pool,
        )
        .await
        .context(RepoFactoryError::SegmentedChangelogManager)?;
        Ok(Arc::new(manager))
    }

    pub fn repo_derived_data(
        &self,
        repo_identity: &ArcRepoIdentity,
        repo_config: &ArcRepoConfig,
        changesets: &ArcChangesets,
        bonsai_hg_mapping: &ArcBonsaiHgMapping,
        filenodes: &ArcFilenodes,
        repo_blobstore: &ArcRepoBlobstore,
    ) -> Result<ArcRepoDerivedData> {
        let config = repo_config.derived_data_config.clone();
        let lease = lease_init(self.env.fb, self.env.caching, DERIVED_DATA_LEASE)?;
        let scuba = build_scuba(
            self.env.fb,
            config.scuba_table.clone(),
            repo_identity.name(),
        )?;
        let derivation_service_client =
            get_derivation_client(self.env.fb, self.env.remote_derivation_options.clone())?;
        Ok(Arc::new(RepoDerivedData::new(
            repo_identity.id(),
            repo_identity.name().to_string(),
            changesets.clone(),
            bonsai_hg_mapping.clone(),
            filenodes.clone(),
            repo_blobstore.as_ref().clone(),
            lease,
            scuba,
            config,
            derivation_service_client,
        )?))
    }

    pub async fn skiplist_index(
        &self,
        repo_config: &ArcRepoConfig,
        repo_identity: &ArcRepoIdentity,
        common_config: &ArcCommonConfig,
    ) -> Result<ArcSkiplistIndex> {
        let blobstore_without_cache = self
            .repo_blobstore_from_blobstore(
                repo_identity,
                repo_config,
                &self
                    .blobstore_no_cache(&repo_config.storage_config.blobstore)
                    .await?,
                common_config,
            )
            .await?;

        let skiplist_key = if self.env.skiplist_enabled {
            repo_config.skiplist_index_blobstore_key.clone()
        } else {
            None
        };

        SkiplistIndex::from_blobstore(
            &self.ctx(Some(repo_identity)),
            &skiplist_key,
            &blobstore_without_cache.boxed(),
        )
        .await
    }

    pub async fn repo_blobstore(
        &self,
        repo_identity: &ArcRepoIdentity,
        repo_config: &ArcRepoConfig,
        common_config: &ArcCommonConfig,
    ) -> Result<ArcRepoBlobstore> {
        let blobstore = self
            .blobstore(&repo_config.storage_config.blobstore)
            .await?;
        Ok(Arc::new(
            self.repo_blobstore_from_blobstore(
                repo_identity,
                repo_config,
                &blobstore,
                common_config,
            )
            .await?,
        ))
    }

    pub fn filestore_config(&self, repo_config: &ArcRepoConfig) -> ArcFilestoreConfig {
        let filestore_config = repo_config.filestore.as_ref().map_or_else(
            FilestoreConfig::no_chunking_filestore,
            |p| FilestoreConfig {
                chunk_size: Some(p.chunk_size),
                concurrency: p.concurrency,
            },
        );
        Arc::new(filestore_config)
    }

    pub async fn redaction_config_blobstore(
        &self,
        common_config: &ArcCommonConfig,
    ) -> Result<ArcRedactionConfigBlobstore> {
        self.redaction_config_blobstore_from_config(&common_config.redaction_config.blobstore)
            .await
    }

    pub async fn repo_ephemeral_store(
        &self,
        sql_query_config: &ArcSqlQueryConfig,
        repo_identity: &ArcRepoIdentity,
        repo_config: &ArcRepoConfig,
    ) -> Result<ArcRepoEphemeralStore> {
        if let Some(ephemeral_config) = &repo_config.storage_config.ephemeral_blobstore {
            let blobstore = self
                .blobstore_enumerable_with_unlink(&ephemeral_config.blobstore)
                .await?;
            let ephemeral_blobstore = RepoEphemeralStoreBuilder::with_database_config(
                self.env.fb,
                &ephemeral_config.metadata,
                &self.env.mysql_options,
                self.env.readonly_storage.0,
            )?
            .build(
                repo_identity.id(),
                blobstore,
                sql_query_config.clone(),
                ephemeral_config.initial_bubble_lifespan,
                ephemeral_config.bubble_expiration_grace,
                ephemeral_config.bubble_deletion_mode,
            );
            Ok(Arc::new(ephemeral_blobstore))
        } else {
            Ok(Arc::new(RepoEphemeralStore::disabled(repo_identity.id())))
        }
    }

    pub async fn mutable_renames(&self, repo_config: &ArcRepoConfig) -> Result<ArcMutableRenames> {
        let sql_store = self
            .open::<SqlMutableRenamesStore>(&repo_config.storage_config.metadata)
            .await
            .context(RepoFactoryError::MutableRenames)?;
        let cache_pool = self.maybe_volatile_pool("mutable_renames")?;
        Ok(Arc::new(MutableRenames::new(
            self.env.fb,
            repo_config.repoid,
            sql_store,
            cache_pool,
        )?))
    }

    pub fn derived_data_manager_set(
        &self,
        repo_identity: &ArcRepoIdentity,
        repo_config: &ArcRepoConfig,
        changesets: &ArcChangesets,
        bonsai_hg_mapping: &ArcBonsaiHgMapping,
        filenodes: &ArcFilenodes,
        repo_blobstore: &ArcRepoBlobstore,
    ) -> Result<ArcDerivedDataManagerSet> {
        let config = repo_config.derived_data_config.clone();
        let lease = lease_init(self.env.fb, self.env.caching, DERIVED_DATA_LEASE)?;
        let ctx = self.ctx(Some(repo_identity));
        let logger = ctx.logger().clone();
        let derived_data_scuba = build_scuba(
            self.env.fb,
            config.scuba_table.clone(),
            repo_identity.name(),
        )?;
        let derivation_service_client =
            get_derivation_client(self.env.fb, self.env.remote_derivation_options.clone())?;
        anyhow::Ok(Arc::new(DerivedDataManagerSet::new(
            repo_identity.id(),
            repo_identity.name().to_string(),
            changesets.clone(),
            bonsai_hg_mapping.clone(),
            filenodes.clone(),
            repo_blobstore.as_ref().clone(),
            lease,
            logger,
            derived_data_scuba,
            config,
            derivation_service_client,
        )?))
    }

    /// The commit mapping bettween repos for synced commits.
    pub async fn synced_commit_mapping(
        &self,
        repo_config: &ArcRepoConfig,
    ) -> Result<ArcSyncedCommitMapping> {
        Ok(Arc::new(
            self.open::<SqlSyncedCommitMapping>(&repo_config.storage_config.metadata)
                .await?,
        ))
    }

    /// Cross-repo sync manager for this repo
    pub async fn repo_cross_repo(
        &self,
        repo_identity: &ArcRepoIdentity,
        synced_commit_mapping: &ArcSyncedCommitMapping,
    ) -> Result<ArcRepoCrossRepo> {
        let sync_lease = create_commit_syncer_lease(self.env.fb, self.env.caching)?;
        let logger = self
            .env
            .logger
            .new(o!("repo" => repo_identity.name().to_string()));
        let live_commit_sync_config = Arc::new(CfgrLiveCommitSyncConfig::new(
            &logger,
            &self.env.config_store,
        )?);

        Ok(Arc::new(RepoCrossRepo::new(
            synced_commit_mapping.clone(),
            live_commit_sync_config,
            sync_lease,
        )))
    }

    pub async fn mutable_counters(
        &self,
        repo_identity: &ArcRepoIdentity,
        repo_config: &ArcRepoConfig,
    ) -> Result<ArcMutableCounters> {
        Ok(Arc::new(
            self.open::<SqlMutableCountersBuilder>(&repo_config.storage_config.metadata)
                .await
                .context(RepoFactoryError::MutableCounters)?
                .build(repo_identity.id()),
        ))
    }

    pub fn acl_regions(
        &self,
        repo_config: &ArcRepoConfig,
        skiplist_index: &ArcSkiplistIndex,
        changeset_fetcher: &ArcChangesetFetcher,
    ) -> ArcAclRegions {
        build_acl_regions(
            repo_config.acl_region_config.as_ref(),
            skiplist_index.clone(),
            changeset_fetcher.clone(),
        )
    }

    pub async fn hook_manager(
        &self,
        repo_config: &ArcRepoConfig,
        repo_identity: &ArcRepoIdentity,
        repo_derived_data: &ArcRepoDerivedData,
        bookmarks: &ArcBookmarks,
        repo_blobstore: &ArcRepoBlobstore,
    ) -> Result<ArcHookManager> {
        let name = repo_identity.name();

        let content_store = RepoFileContentManager::from_parts(
            bookmarks.clone(),
            repo_blobstore.clone(),
            repo_derived_data.clone(),
        );

        let disabled_hooks = self
            .env
            .disabled_hooks
            .get(name)
            .cloned()
            .unwrap_or_default();

        let hooks_scuba_local_path = repo_config.scuba_local_path_hooks.clone();

        let mut hooks_scuba = MononokeScubaSampleBuilder::with_opt_table(
            self.env.fb,
            repo_config.scuba_table_hooks.clone(),
        )?;

        hooks_scuba.add("repo", name);

        if let Some(hooks_scuba_local_path) = hooks_scuba_local_path {
            hooks_scuba = hooks_scuba.with_log_file(hooks_scuba_local_path)?;
        }

        let hook_manager = async {
            let fetcher = Box::new(TextOnlyFileContentManager::new(
                content_store,
                repo_config.hook_max_file_size,
            ));

            let mut hook_manager = HookManager::new(
                self.env.fb,
                self.env.acl_provider.as_ref(),
                fetcher,
                repo_config.hook_manager_params.clone().unwrap_or_default(),
                hooks_scuba,
                name.to_string(),
            )
            .await?;

            load_hooks(
                self.env.fb,
                self.env.acl_provider.as_ref(),
                &mut hook_manager,
                repo_config,
                &disabled_hooks,
            )
            .await?;

            <Result<_, anyhow::Error>>::Ok(hook_manager)
        }
        .watched(&self.env.logger)
        .await
        .context(RepoFactoryError::HookManager)?;

        Ok(Arc::new(hook_manager))
    }

    pub async fn sparse_profiles(
        &self,
        repo_config: &ArcRepoConfig,
    ) -> Result<ArcRepoSparseProfiles> {
        let sql = self
            .sql_factory(&repo_config.storage_config.metadata)
            .await?
            .open::<SqlSparseProfilesSizes>()
            .ok();
        Ok(Arc::new(RepoSparseProfiles {
            sql_profile_sizes: sql,
        }))
    }

    pub fn repo_lock(
        &self,
        repo_config: &ArcRepoConfig,
        repo_identity: &ArcRepoIdentity,
    ) -> Result<ArcRepoLock> {
        match repo_config.readonly {
            RepoReadOnly::ReadOnly(ref reason) => Ok(Arc::new(AlwaysLockedRepoLock::new(
                repo_identity.id(),
                reason.clone(),
            ))),
            RepoReadOnly::ReadWrite => {
                let sql = SqlRepoLock::with_metadata_database_config(
                    self.env.fb,
                    &repo_config.storage_config.metadata,
                    &self.env.mysql_options,
                    self.env.readonly_storage.0,
                )?;

                Ok(Arc::new(MutableRepoLock::new(sql, repo_identity.id())))
            }
        }
    }

    pub async fn repo_bookmark_attrs(
        &self,
        repo_config: &ArcRepoConfig,
    ) -> Result<ArcRepoBookmarkAttrs> {
        let repo_bookmark_attrs = RepoBookmarkAttrs::new(
            self.env.acl_provider.as_ref(),
            repo_config.bookmarks.clone(),
        )
        .await
        .context(RepoFactoryError::RepoBookmarkAttrs)?;
        Ok(Arc::new(repo_bookmark_attrs))
    }

    pub async fn streaming_clone(
        &self,
        repo_config: &ArcRepoConfig,
        repo_identity: &ArcRepoIdentity,
        repo_blobstore: &ArcRepoBlobstore,
    ) -> Result<ArcStreamingClone> {
        let streaming_clone = self
            .open::<StreamingCloneBuilder>(&repo_config.storage_config.metadata)
            .await
            .context(RepoFactoryError::StreamingClone)?
            .build(repo_identity.id(), repo_blobstore.clone());
        Ok(Arc::new(streaming_clone))
    }

    pub async fn warm_bookmarks_cache(
        &self,
        bookmarks: &ArcBookmarks,
        bookmark_update_log: &ArcBookmarkUpdateLog,
        repo_identity: &ArcRepoIdentity,
        repo_derived_data: &ArcRepoDerivedData,
        phases: &ArcPhases,
    ) -> Result<ArcBookmarksCache> {
        match self.env.warm_bookmarks_cache_derived_data {
            Some(derived_data) => {
                let mut scuba = self.env.warm_bookmarks_cache_scuba_sample_builder.clone();
                scuba.add("repo", repo_identity.name());

                let mut wbc_builder = WarmBookmarksCacheBuilder::new(
                    self.ctx(Some(repo_identity)),
                    bookmarks.clone(),
                    bookmark_update_log.clone(),
                    repo_identity.clone(),
                );

                match derived_data {
                    WarmBookmarksCacheDerivedData::HgOnly => {
                        wbc_builder.add_hg_warmers(repo_derived_data, phases)?;
                    }
                    WarmBookmarksCacheDerivedData::AllKinds => {
                        wbc_builder.add_all_warmers(repo_derived_data, phases)?;
                    }
                    WarmBookmarksCacheDerivedData::None => {}
                }

                Ok(Arc::new(
                    wbc_builder.build().watched(&self.env.logger).await?,
                ))
            }
            None => Ok(Arc::new(NoopBookmarksCache::new(bookmarks.clone()))),
        }
    }

    pub async fn target_repo_dbs(
        &self,
        repo_config: &ArcRepoConfig,
        bookmarks: &ArcBookmarks,
        bookmark_update_log: &ArcBookmarkUpdateLog,
        mutable_counters: &ArcMutableCounters,
    ) -> Result<ArcTargetRepoDbs> {
        let connections: SqlConnections = self
            .sql_connections_with_label(
                &repo_config.storage_config.metadata,
                "bookmark_mutable_counters",
            )
            .await
            .context(RepoFactoryError::TargetRepoDbs)?
            .into();
        let target_repo_dbs = TargetRepoDbs {
            connections,
            bookmarks: bookmarks.clone(),
            bookmark_update_log: bookmark_update_log.clone(),
            counters: mutable_counters.clone(),
        };
        Ok(Arc::new(target_repo_dbs))
    }

    pub async fn push_redirector_mode(
        &self,
        repo_identity: &ArcRepoIdentity,
        repo_cross_repo: &ArcRepoCrossRepo,
        target_repo_dbs: &ArcTargetRepoDbs,
    ) -> Result<ArcPushRedirectorMode> {
        let common_commit_sync_config = repo_cross_repo
            .live_commit_sync_config()
            .clone()
            .get_common_config_if_exists(repo_identity.id())
            .context(RepoFactoryError::PushRedirectorBase)?;
        let synced_commit_mapping = repo_cross_repo.synced_commit_mapping();

        let push_redirector_mode = match common_commit_sync_config {
            Some(common_commit_sync_config)
                if common_commit_sync_config.large_repo_id != repo_identity.id() =>
            {
                PushRedirectorMode::Enabled(Arc::new(PushRedirectorBase {
                    common_commit_sync_config,
                    synced_commit_mapping: synced_commit_mapping.clone(),
                    target_repo_dbs: target_repo_dbs.clone(),
                }))
            }
            _ => PushRedirectorMode::Disabled,
        };

        Ok(Arc::new(push_redirector_mode))
    }

    pub async fn repo_handler_base(
        &self,
        repo_identity: &ArcRepoIdentity,
        repo_config: &ArcRepoConfig,
        push_redirector_mode: &ArcPushRedirectorMode,
    ) -> Result<ArcRepoHandlerBase> {
        let ctx = self.ctx(Some(repo_identity));
        let scuba = ctx.scuba().clone();
        let logger = ctx.logger().clone();
        let repo_client_knobs = repo_config.repo_client_knobs.clone();
        let backup_repo_config = repo_config.backup_repo_config.clone();
        let maybe_push_redirector_base = match **push_redirector_mode {
            Enabled(ref push_redirector_base) => Some(Arc::clone(push_redirector_base)),
            PushRedirectorMode::Disabled => None,
        };
        Ok(Arc::new(RepoHandlerBase {
            logger,
            scuba,
            maybe_push_redirector_base,
            repo_client_knobs,
            backup_repo_config,
        }))
    }

    pub async fn sql_query_config(&self) -> Result<ArcSqlQueryConfig> {
        let caching = if matches!(self.env.caching, Caching::Enabled(_)) {
            const KEY_PREFIX: &str = "scm.mononoke.sql";
            const MC_CODEVER: u32 = 0;
            let sitever: u32 = tunables()
                .get_sql_memcache_sitever()
                .try_into()
                .unwrap_or(0);
            Some(sql_query_config::CachingConfig {
                keygen: KeyGen::new(KEY_PREFIX, MC_CODEVER, sitever),
                memcache: MemcacheClient::new(self.env.fb)?.into(),
                cache_pool: volatile_pool("sql")?,
            })
        } else {
            None
        };
        Ok(Arc::new(SqlQueryConfig { caching }))
    }
}

fn lease_init(
    fb: FacebookInit,
    caching: Caching,
    lease_type: &'static str,
) -> Result<Arc<dyn LeaseOps>> {
    // Derived data leasing is performed through the cache, so is only
    // available if caching is enabled.
    if let Caching::Enabled(_) = caching {
        Ok(Arc::new(MemcacheOps::new(fb, lease_type, "")?))
    } else {
        Ok(Arc::new(InProcessLease::new()))
    }
}

fn build_scuba(
    fb: FacebookInit,
    scuba_table: Option<String>,
    reponame: &str,
) -> Result<MononokeScubaSampleBuilder> {
    let mut scuba = MononokeScubaSampleBuilder::with_opt_table(fb, scuba_table)?;
    scuba.add_common_server_data();
    scuba.add("reponame", reponame);
    Ok(scuba)
}

fn get_derivation_client(
    fb: FacebookInit,
    remote_derivation_options: RemoteDerivationOptions,
) -> Result<Option<Arc<dyn DerivationClient>>> {
    let derivation_service_client: Option<Arc<dyn DerivationClient>> =
        if remote_derivation_options.derive_remotely {
            #[cfg(fbcode_build)]
            {
                let client = match remote_derivation_options.smc_tier {
                    Some(smc_tier) => DerivationServiceClient::from_tier_name(fb, smc_tier)?,
                    None => DerivationServiceClient::new(fb)?,
                };
                Some(Arc::new(client))
            }
            #[cfg(not(fbcode_build))]
            {
                let _ = fb;
                None
            }
        } else {
            None
        };
    Ok(derivation_service_client)
}
