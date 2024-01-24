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
use blobstore::BlobstoreUnlinkOps;
use blobstore_factory::default_scrub_handler;
use blobstore_factory::make_blobstore;
use blobstore_factory::make_blobstore_enumerable_with_unlink;
use blobstore_factory::make_blobstore_unlink_ops;
pub use blobstore_factory::BlobstoreOptions;
use blobstore_factory::ComponentSamplingHandler;
use blobstore_factory::MetadataSqlFactory;
pub use blobstore_factory::ReadOnlyStorage;
use blobstore_factory::ScrubHandler;
use bonsai_blob_mapping::ArcBonsaiBlobMapping;
use bonsai_blob_mapping::BonsaiBlobMapping;
use bonsai_blob_mapping::SqlBonsaiBlobMapping;
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
use bonsai_tag_mapping::ArcBonsaiTagMapping;
use bonsai_tag_mapping::SqlBonsaiTagMappingBuilder;
#[cfg(fbcode_build)]
use bookmark_service_client::BookmarkServiceClient;
#[cfg(fbcode_build)]
use bookmark_service_client::RepoBookmarkServiceClient;
use bookmarks::bookmark_heads_fetcher;
use bookmarks::ArcBookmarkUpdateLog;
use bookmarks::ArcBookmarks;
use bookmarks::CachedBookmarks;
use bookmarks_cache::ArcBookmarksCache;
use cacheblob::new_cachelib_blobstore_no_lease;
use cacheblob::new_memcache_blobstore;
use cacheblob::CachelibBlobstoreOptions;
use cacheblob::InProcessLease;
use cacheblob::LeaseOps;
use cacheblob::MemcacheOps;
use caching_commit_graph_storage::CachingCommitGraphStorage;
use caching_ext::CacheHandlerFactory;
use changeset_fetcher::ArcChangesetFetcher;
use changeset_fetcher::SimpleChangesetFetcher;
use changesets::ArcChangesets;
use changesets_impl::CachingChangesets;
use changesets_impl::SqlChangesetsBuilder;
use commit_graph::ArcCommitGraph;
use commit_graph::CommitGraph;
use commit_graph_compat::ChangesetsCommitGraphCompat;
use commit_graph_types::storage::CommitGraphStorage;
use context::CoreContext;
use context::SessionContainer;
use cross_repo_sync::create_commit_syncer_lease;
use dbbookmarks::ArcSqlBookmarks;
use dbbookmarks::SqlBookmarksBuilder;
use deletion_log::ArcDeletionLog;
use deletion_log::DeletionLog;
use deletion_log::SqlDeletionLog;
#[cfg(fbcode_build)]
use derived_data_client_library::Client as DerivationServiceClient;
#[cfg(fbcode_build)]
use derived_data_remote::Address;
use derived_data_remote::DerivationClient;
use derived_data_remote::RemoteDerivationOptions;
#[cfg(fbcode_build)]
use environment::BookmarkCacheAddress;
use environment::BookmarkCacheDerivedData;
use environment::BookmarkCacheKind;
use environment::Caching;
use environment::MononokeEnvironment;
use ephemeral_blobstore::ArcRepoEphemeralStore;
use ephemeral_blobstore::RepoEphemeralStore;
use ephemeral_blobstore::RepoEphemeralStoreBuilder;
use fbinit::FacebookInit;
use filenodes::ArcFilenodes;
use filestore::ArcFilestoreConfig;
use filestore::FilestoreConfig;
use futures_watchdog::WatchdogExt;
use git_symbolic_refs::ArcGitSymbolicRefs;
use git_symbolic_refs::SqlGitSymbolicRefsBuilder;
use hook_manager::manager::ArcHookManager;
use hook_manager::manager::HookManager;
use hook_manager::TextOnlyHookFileContentProvider;
use hooks::hook_loader::load_hooks;
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
use preloaded_commit_graph_storage::PreloadedCommitGraphStorage;
use pushrebase_mutation_mapping::ArcPushrebaseMutationMapping;
use pushrebase_mutation_mapping::SqlPushrebaseMutationMappingConnection;
use readonlyblob::ReadOnlyBlobstore;
use redactedblobstore::ArcRedactionConfigBlobstore;
use redactedblobstore::RedactedBlobs;
use redactedblobstore::RedactionConfigBlobstore;
use repo_blobstore::ArcRepoBlobstore;
use repo_blobstore::ArcRepoBlobstoreUnlinkOps;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreUnlinkOps;
use repo_bookmark_attrs::ArcRepoBookmarkAttrs;
use repo_bookmark_attrs::RepoBookmarkAttrs;
use repo_cross_repo::ArcRepoCrossRepo;
use repo_cross_repo::RepoCrossRepo;
use repo_derived_data::ArcRepoDerivedData;
use repo_derived_data::RepoDerivedData;
use repo_derived_data_service::ArcDerivedDataManagerSet;
use repo_derived_data_service::DerivedDataManagerSet;
use repo_hook_file_content_provider::RepoHookFileContentProvider;
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
use slog::o;
use sql_commit_graph_storage::SqlCommitGraphStorageBuilder;
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
use virtually_sharded_blobstore::VirtuallyShardedBlobstore;
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
    blobstores: RepoFactoryCache<BlobConfig, Arc<dyn Blobstore>>,
    redacted_blobs: RepoFactoryCache<MetadataDatabaseConfig, Arc<RedactedBlobs>>,
    blobstore_override: Option<Arc<dyn RepoFactoryOverride<Arc<dyn Blobstore>>>>,
    lease_override: Option<Arc<dyn RepoFactoryOverride<Arc<dyn LeaseOps>>>>,
    scrub_handler: Arc<dyn ScrubHandler>,
    blobstore_component_sampler: Option<Arc<dyn ComponentSamplingHandler>>,
    bonsai_hg_mapping_overwrite: bool,
}

impl RepoFactory {
    pub fn new(env: Arc<MononokeEnvironment>) -> RepoFactory {
        RepoFactory {
            sql_factories: RepoFactoryCache::new(),
            blobstores: RepoFactoryCache::new(),
            redacted_blobs: RepoFactoryCache::new(),
            blobstore_override: None,
            lease_override: None,
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

    pub fn with_lease_override(
        &mut self,
        lease_override: impl RepoFactoryOverride<Arc<dyn LeaseOps>>,
    ) -> &mut Self {
        self.lease_override = Some(Arc::new(lease_override));
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
                let sql_factory = MetadataSqlFactory::new(
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

    async fn open_sql<T: SqlConstructFromMetadataDatabaseConfig>(
        &self,
        config: &RepoConfig,
    ) -> Result<T> {
        let sql_factory = self.sql_factory(&config.storage_config.metadata).await?;
        sql_factory.open::<T>().await
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

    async fn repo_blobstore_unlink_ops_from_blobstore_unlink_ops(
        &self,
        repo_identity: &ArcRepoIdentity,
        repo_config: &ArcRepoConfig,
        blobstore: &Arc<dyn BlobstoreUnlinkOps>,
        common_config: &ArcCommonConfig,
    ) -> Result<RepoBlobstoreUnlinkOps> {
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

        let repo_blobstore = RepoBlobstoreUnlinkOps::new(
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
                    Caching::Enabled(local_cache_config) => {
                        let fb = self.env.fb;
                        let memcache_blobstore = tokio::task::spawn_blocking(move || {
                            new_memcache_blobstore(fb, blobstore, "multiplexed", "")
                        })
                        .await??;
                        blobstore = cachelib_blobstore(
                            memcache_blobstore,
                            local_cache_config.blobstore_cache_shards,
                            &self.env.blobstore_options.cachelib_options,
                        )?
                    }
                    Caching::LocalOnly(local_cache_config) => {
                        blobstore = cachelib_blobstore(
                            blobstore,
                            local_cache_config.blobstore_cache_shards,
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

    fn lease(&self, lease_type: &'static str) -> Result<Arc<dyn LeaseOps>> {
        let fb = self.env.fb;
        let caching = self.env.caching;
        Self::lease_init(fb, caching, lease_type).map(|lease| {
            if let Some(lease_override) = &self.lease_override {
                lease_override(lease)
            } else {
                lease
            }
        })
    }

    pub async fn blobstore_unlink_ops_with_overriden_blob_config(
        &self,
        config: &BlobConfig,
    ) -> Result<Arc<dyn BlobstoreUnlinkOps>> {
        make_blobstore_unlink_ops(
            self.env.fb,
            config.clone(),
            &self.env.mysql_options,
            self.env.readonly_storage,
            &self.env.blobstore_options,
            &self.env.logger,
            &self.env.config_store,
            &self.scrub_handler,
            self.blobstore_component_sampler.as_ref(),
            None,
        )
        .watched(&self.env.logger)
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
                let blobstore = self.redaction_config_blobstore(common_config).await?;
                Ok(Arc::new(
                    RedactedBlobs::from_configerator(
                        &self.env.config_store,
                        &common_config.redaction_config.redaction_sets_location,
                        ctx,
                        blobstore,
                    )
                    .await?,
                ))
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

    /// Returns a cache builder for the named pool if caching is enabled
    fn cache_handler_factory(&self, name: &str) -> Result<Option<CacheHandlerFactory>> {
        match self.env.caching {
            Caching::Enabled(_) => Ok(Some(CacheHandlerFactory::Shared {
                cachelib_pool: volatile_pool(name)?,
                memcache_client: MemcacheClient::new(self.env.fb)
                    .context("Failed to initialize memcache client")?,
            })),
            Caching::LocalOnly(_) => Ok(Some(CacheHandlerFactory::Local {
                cachelib_pool: volatile_pool(name)?,
            })),
            Caching::Disabled => Ok(None),
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

    #[error("Error opening bonsai-tag mapping")]
    BonsaiTagMapping,

    #[error("Error opening git-symbolic-refs")]
    GitSymbolicRefs,

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

    #[error("Error openning bonsai blob mapping DB")]
    BonsaiBlobMapping,

    #[error("Error openning deletion log DB")]
    SqlDeletionLog,
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
        commit_graph: &ArcCommitGraph,
    ) -> Result<ArcChangesets> {
        let builder = self
            .open_sql::<SqlChangesetsBuilder>(repo_config)
            .await
            .context(RepoFactoryError::Changesets)?;
        let changesets = builder.build(self.env.rendezvous_options, repo_identity.id());

        let possibly_cached_changesets: ArcChangesets =
            if let Some(cache_handler_factory) = self.cache_handler_factory("changesets")? {
                Arc::new(CachingChangesets::new(
                    Arc::new(changesets),
                    cache_handler_factory,
                ))
            } else {
                Arc::new(changesets)
            };

        Ok(Arc::new(ChangesetsCommitGraphCompat::new(
            self.env.fb,
            possibly_cached_changesets,
            commit_graph.clone(),
            repo_identity.name().to_string(),
            repo_config.commit_graph_config.scuba_table.as_deref(),
        )?))
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
            .open_sql::<SqlBookmarksBuilder>(repo_config)
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
            .open_sql::<SqlPhasesBuilder>(repo_config)
            .await
            .context(RepoFactoryError::Phases)?;
        if let Some(cache_handler_factory) = self.cache_handler_factory("phases")? {
            sql_phases_builder.enable_caching(cache_handler_factory);
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
            .open_sql::<SqlBonsaiHgMappingBuilder>(repo_config)
            .await
            .context(RepoFactoryError::BonsaiHgMapping)?;

        if self.bonsai_hg_mapping_overwrite {
            builder = builder.with_overwrite();
        }

        let bonsai_hg_mapping = builder.build(repo_identity.id(), self.env.rendezvous_options);

        if let Some(cache_handler_factory) = self.cache_handler_factory("bonsai_hg_mapping")? {
            Ok(Arc::new(CachingBonsaiHgMapping::new(
                Arc::new(bonsai_hg_mapping),
                cache_handler_factory,
            )?))
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
            .open_sql::<SqlBonsaiGitMappingBuilder>(repo_config)
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
            .open_sql::<SqlLongRunningRequestsQueue>(repo_config)
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
            .open_sql::<SqlBonsaiGlobalrevMappingBuilder>(repo_config)
            .await
            .context(RepoFactoryError::BonsaiGlobalrevMapping)?
            .build(repo_identity.id());
        if let Some(cache_handler_factory) =
            self.cache_handler_factory("bonsai_globalrev_mapping")?
        {
            Ok(Arc::new(CachingBonsaiGlobalrevMapping::new(
                Arc::new(bonsai_globalrev_mapping),
                cache_handler_factory,
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
            .open_sql::<SqlBonsaiSvnrevMappingBuilder>(repo_config)
            .await
            .context(RepoFactoryError::BonsaiSvnrevMapping)?
            .build(repo_identity.id());
        if let Some(cache_handler_factory) = self.cache_handler_factory("bonsai_svnrev_mapping")? {
            Ok(Arc::new(CachingBonsaiSvnrevMapping::new(
                Arc::new(bonsai_svnrev_mapping),
                cache_handler_factory,
            )))
        } else {
            Ok(Arc::new(bonsai_svnrev_mapping))
        }
    }

    pub async fn bonsai_tag_mapping(
        &self,
        repo_config: &ArcRepoConfig,
        repo_identity: &ArcRepoIdentity,
    ) -> Result<ArcBonsaiTagMapping> {
        let bonsai_tag_mapping = self
            .open_sql::<SqlBonsaiTagMappingBuilder>(repo_config)
            .await
            .context(RepoFactoryError::BonsaiTagMapping)?
            .build(repo_identity.id());
        // Caching is not enabled for now, but can be added later if required.
        Ok(Arc::new(bonsai_tag_mapping))
    }

    pub async fn git_symbolic_refs(
        &self,
        repo_config: &ArcRepoConfig,
        repo_identity: &ArcRepoIdentity,
    ) -> Result<ArcGitSymbolicRefs> {
        let git_symbolic_refs = self
            .open_sql::<SqlGitSymbolicRefsBuilder>(repo_config)
            .await
            .context(RepoFactoryError::GitSymbolicRefs)?
            .build(repo_identity.id());
        // Caching is not enabled for now, but can be added later if required.
        Ok(Arc::new(git_symbolic_refs))
    }

    pub async fn pushrebase_mutation_mapping(
        &self,
        repo_config: &ArcRepoConfig,
    ) -> Result<ArcPushrebaseMutationMapping> {
        let conn = self
            .open_sql::<SqlPushrebaseMutationMappingConnection>(repo_config)
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
        let mut filenodes_builder = sql_factory
            .open_shardable::<NewFilenodesBuilder>()
            .await
            .context(RepoFactoryError::Filenodes)?;
        if let (Some(filenodes_cache_handler_factory), Some(history_cache_handler_factory)) = (
            self.cache_handler_factory("filenodes")?,
            self.cache_handler_factory("filenodes_history")?,
        ) {
            let filenodes_tier = sql_factory.tier_info_shardable::<NewFilenodesBuilder>()?;
            filenodes_builder.enable_caching(
                filenodes_cache_handler_factory,
                history_cache_handler_factory,
                "newfilenodes",
                &filenodes_tier.tier_name,
            )?;
        }
        Ok(Arc::new(filenodes_builder.build(repo_identity.id())?))
    }

    pub async fn hg_mutation_store(
        &self,
        repo_config: &ArcRepoConfig,
        repo_identity: &ArcRepoIdentity,
    ) -> Result<ArcHgMutationStore> {
        let hg_mutation_store = self
            .open_sql::<SqlHgMutationStoreBuilder>(repo_config)
            .await
            .context(RepoFactoryError::HgMutationStore)?
            .with_repo_id(repo_identity.id());

        if let Some(cache_handler_factory) = self.cache_handler_factory("hg_mutation_store")? {
            Ok(Arc::new(CachedHgMutationStore::new(
                Arc::new(hg_mutation_store),
                cache_handler_factory,
            )?))
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
            .open_sql::<SegmentedChangelogSqlConnections>(repo_config)
            .await
            .context(RepoFactoryError::SegmentedChangelog)?;
        let cache_handler_factory = self.cache_handler_factory("segmented_changelog")?;
        let segmented_changelog = new_server_segmented_changelog(
            &self.ctx(Some(repo_identity)),
            repo_identity,
            repo_config.segmented_changelog_config.clone(),
            sql_connections,
            changeset_fetcher.clone(),
            bookmarks.clone(),
            repo_blobstore.clone(),
            cache_handler_factory,
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
            .open_sql::<SegmentedChangelogSqlConnections>(repo_config)
            .await
            .context(RepoFactoryError::SegmentedChangelogManager)?;
        let cache_handler_factory = self.cache_handler_factory("segmented_changelog")?;
        let manager = new_server_segmented_changelog_manager(
            &self.ctx(Some(repo_identity)),
            repo_identity,
            repo_config.segmented_changelog_config.clone(),
            sql_connections,
            changeset_fetcher.clone(),
            bookmarks.clone(),
            repo_blobstore.clone(),
            cache_handler_factory,
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
        commit_graph: &ArcCommitGraph,
        bonsai_hg_mapping: &ArcBonsaiHgMapping,
        bonsai_git_mapping: &ArcBonsaiGitMapping,
        filenodes: &ArcFilenodes,
        repo_blobstore: &ArcRepoBlobstore,
    ) -> Result<ArcRepoDerivedData> {
        let config = repo_config.derived_data_config.clone();
        let lease = self.lease(DERIVED_DATA_LEASE)?;
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
            commit_graph.clone(),
            bonsai_hg_mapping.clone(),
            bonsai_git_mapping.clone(),
            filenodes.clone(),
            repo_blobstore.as_ref().clone(),
            lease,
            scuba,
            config,
            derivation_service_client,
        )?))
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

    pub async fn repo_blobstore_unlink_ops(
        &self,
        repo_identity: &ArcRepoIdentity,
        repo_config: &ArcRepoConfig,
        common_config: &ArcCommonConfig,
    ) -> Result<ArcRepoBlobstoreUnlinkOps> {
        let blobstore = self
            .blobstore_unlink_ops_with_overriden_blob_config(&repo_config.storage_config.blobstore)
            .await?;
        Ok(Arc::new(
            self.repo_blobstore_unlink_ops_from_blobstore_unlink_ops(
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
            .open_sql::<SqlMutableRenamesStore>(repo_config)
            .await
            .context(RepoFactoryError::MutableRenames)?;
        let cache_handler_factory = self.cache_handler_factory("mutable_renames")?;
        Ok(Arc::new(MutableRenames::new(
            repo_config.repoid,
            sql_store,
            cache_handler_factory,
        )?))
    }

    pub fn derived_data_manager_set(
        &self,
        repo_identity: &ArcRepoIdentity,
        repo_config: &ArcRepoConfig,
        changesets: &ArcChangesets,
        commit_graph: &ArcCommitGraph,
        bonsai_hg_mapping: &ArcBonsaiHgMapping,
        bonsai_git_mapping: &ArcBonsaiGitMapping,
        filenodes: &ArcFilenodes,
        repo_blobstore: &ArcRepoBlobstore,
    ) -> Result<ArcDerivedDataManagerSet> {
        let config = repo_config.derived_data_config.clone();
        let lease = self.lease(DERIVED_DATA_LEASE)?;
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
            commit_graph.clone(),
            bonsai_hg_mapping.clone(),
            bonsai_git_mapping.clone(),
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
            self.open_sql::<SqlSyncedCommitMapping>(repo_config).await?,
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
            self.open_sql::<SqlMutableCountersBuilder>(repo_config)
                .await
                .context(RepoFactoryError::MutableCounters)?
                .build(repo_identity.id()),
        ))
    }

    pub fn acl_regions(
        &self,
        repo_config: &ArcRepoConfig,
        commit_graph: &ArcCommitGraph,
    ) -> ArcAclRegions {
        build_acl_regions(repo_config.acl_region_config.as_ref(), commit_graph.clone())
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
            let content_provider = Box::new(TextOnlyHookFileContentProvider::new(
                RepoHookFileContentProvider::from_parts(
                    bookmarks.clone(),
                    repo_blobstore.clone(),
                    repo_derived_data.clone(),
                ),
                repo_config.hook_max_file_size,
            ));

            let mut hook_manager = HookManager::new(
                self.env.fb,
                self.env.acl_provider.as_ref(),
                content_provider,
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
        let sql_profile_sizes = self
            .open_sql::<SqlSparseProfilesSizes>(repo_config)
            .await
            .ok();
        Ok(Arc::new(RepoSparseProfiles { sql_profile_sizes }))
    }

    pub async fn repo_lock(
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
                )
                .await?;

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
            .open_sql::<StreamingCloneBuilder>(repo_config)
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
        match &self.env.bookmark_cache_options.cache_kind {
            BookmarkCacheKind::Local => {
                let mut scuba = self.env.warm_bookmarks_cache_scuba_sample_builder.clone();
                scuba.add("repo", repo_identity.name());

                let mut wbc_builder = WarmBookmarksCacheBuilder::new(
                    self.ctx(Some(repo_identity)),
                    bookmarks.clone(),
                    bookmark_update_log.clone(),
                    repo_identity.clone(),
                );

                match self.env.bookmark_cache_options.derived_data {
                    BookmarkCacheDerivedData::HgOnly => {
                        wbc_builder.add_hg_warmers(repo_derived_data, phases)?;
                    }
                    BookmarkCacheDerivedData::AllKinds => {
                        wbc_builder.add_all_warmers(repo_derived_data, phases)?;
                    }
                    BookmarkCacheDerivedData::NoDerivation => {}
                }

                Ok(Arc::new(
                    wbc_builder.build().watched(&self.env.logger).await?,
                ))
            }
            #[cfg(fbcode_build)]
            BookmarkCacheKind::Remote(address) => {
                anyhow::ensure!(
                    self.env.bookmark_cache_options.derived_data
                        == BookmarkCacheDerivedData::HgOnly,
                    "HgOnly derivation supported right now"
                );

                let client = match address {
                    BookmarkCacheAddress::HostPort(host_port) => {
                        BookmarkServiceClient::from_host_port(self.env.fb, host_port.to_string())?
                    }
                    BookmarkCacheAddress::SmcTier(tier_name) => {
                        BookmarkServiceClient::from_tier_name(self.env.fb, tier_name.to_string())?
                    }
                };
                let repo_client =
                    RepoBookmarkServiceClient::new(repo_identity.name().to_string(), client);

                Ok(Arc::new(repo_client))
            }
            #[cfg(not(fbcode_build))]
            BookmarkCacheKind::Remote(_addr) => {
                return Err(anyhow::anyhow!(
                    "Remote bookmark cache not supported in non-fbcode builds"
                ));
            }
            BookmarkCacheKind::Disabled => Ok(Arc::new(NoopBookmarksCache::new(bookmarks.clone()))),
        }
    }

    pub async fn target_repo_dbs(
        &self,
        bookmarks: &ArcBookmarks,
        bookmark_update_log: &ArcBookmarkUpdateLog,
        mutable_counters: &ArcMutableCounters,
    ) -> Result<ArcTargetRepoDbs> {
        let target_repo_dbs = TargetRepoDbs {
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
        let caching = if let Some(cache_handler_factory) = self.cache_handler_factory("sql")? {
            const KEY_PREFIX: &str = "scm.mononoke.sql";
            const MC_CODEVER: u32 = 0;
            let sitever = justknobs::get_as::<u32>("scm/mononoke_memcache_sitevers:sql", None)?;
            Some(sql_query_config::CachingConfig {
                keygen: KeyGen::new(KEY_PREFIX, MC_CODEVER, sitever),
                cache_handler_factory,
            })
        } else {
            None
        };
        Ok(Arc::new(SqlQueryConfig { caching }))
    }

    pub async fn commit_graph(
        &self,
        repo_identity: &ArcRepoIdentity,
        repo_config: &ArcRepoConfig,
        common_config: &ArcCommonConfig,
    ) -> Result<ArcCommitGraph> {
        let sql_storage = self
            .open_sql::<SqlCommitGraphStorageBuilder>(repo_config)
            .await?
            .build(self.env.rendezvous_options, repo_identity.id());

        let maybe_cached_storage: Arc<dyn CommitGraphStorage> =
            if let Some(cache_handler_factory) = self.cache_handler_factory("commit_graph")? {
                Arc::new(CachingCommitGraphStorage::new(
                    Arc::new(sql_storage),
                    cache_handler_factory,
                ))
            } else {
                Arc::new(sql_storage)
            };

        match &repo_config
            .commit_graph_config
            .preloaded_commit_graph_blobstore_key
        {
            Some(preloaded_commit_graph_key) => {
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

                let preloaded_commit_graph_storage = PreloadedCommitGraphStorage::from_blobstore(
                    &self.ctx(Some(repo_identity)),
                    repo_identity.id(),
                    Arc::new(blobstore_without_cache),
                    preloaded_commit_graph_key.clone(),
                    maybe_cached_storage,
                )
                .await?;

                Ok(Arc::new(CommitGraph::new(preloaded_commit_graph_storage)))
            }
            None => Ok(Arc::new(CommitGraph::new(maybe_cached_storage))),
        }
    }

    pub async fn bonsai_blob_mapping(
        &self,
        repo_config: &ArcRepoConfig,
    ) -> Result<ArcBonsaiBlobMapping> {
        let sql_factory = self
            .sql_factory(&repo_config.storage_config.metadata)
            .await?;
        let sql_bonsai_blob_mapping = sql_factory
            .open_shardable::<SqlBonsaiBlobMapping>()
            .await
            .context(RepoFactoryError::BonsaiBlobMapping)?;
        Ok(Arc::new(BonsaiBlobMapping {
            sql_bonsai_blob_mapping,
        }))
    }

    pub async fn deletion_log(&self, repo_config: &ArcRepoConfig) -> Result<ArcDeletionLog> {
        let sql_deletion_log = self
            .open_sql::<SqlDeletionLog>(repo_config)
            .await
            .context(RepoFactoryError::SqlDeletionLog)?;
        Ok(Arc::new(DeletionLog { sql_deletion_log }))
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
                let client = match remote_derivation_options.address {
                    Address::SmcTier(smc_tier) => {
                        DerivationServiceClient::from_tier_name(fb, smc_tier)?
                    }
                    Address::HostPort(host_port) => {
                        DerivationServiceClient::from_host_port(fb, host_port)?
                    }
                    Address::Empty => DerivationServiceClient::new(fb)?,
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
