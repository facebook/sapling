/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Repository factory for tests.
#![deny(missing_docs)]

use std::sync::Arc;

use acl_regions::build_acl_regions;
use acl_regions::ArcAclRegions;
use anyhow::Result;
use basename_suffix_skeleton_manifest::RootBasenameSuffixSkeletonManifest;
use blame::BlameRoot;
use blobstore::Blobstore;
use bonsai_git_mapping::ArcBonsaiGitMapping;
use bonsai_git_mapping::SqlBonsaiGitMappingBuilder;
use bonsai_globalrev_mapping::ArcBonsaiGlobalrevMapping;
use bonsai_globalrev_mapping::SqlBonsaiGlobalrevMappingBuilder;
use bonsai_hg_mapping::ArcBonsaiHgMapping;
use bonsai_hg_mapping::SqlBonsaiHgMappingBuilder;
use bonsai_svnrev_mapping::ArcBonsaiSvnrevMapping;
use bonsai_svnrev_mapping::SqlBonsaiSvnrevMappingBuilder;
use bookmarks::bookmark_heads_fetcher;
use bookmarks::ArcBookmarkUpdateLog;
use bookmarks::ArcBookmarks;
use bookmarks::BookmarkName;
use cacheblob::InProcessLease;
use cacheblob::LeaseOps;
use changeset_fetcher::ArcChangesetFetcher;
use changeset_fetcher::SimpleChangesetFetcher;
use changeset_info::ChangesetInfo;
use changesets::ArcChangesets;
use changesets_impl::SqlChangesetsBuilder;
use context::CoreContext;
use dbbookmarks::ArcSqlBookmarks;
use dbbookmarks::SqlBookmarksBuilder;
use deleted_manifest::RootDeletedManifestV2Id;
use derived_data_filenodes::FilenodesOnlyPublic;
use derived_data_manager::BonsaiDerivable;
use ephemeral_blobstore::ArcRepoEphemeralStore;
use ephemeral_blobstore::RepoEphemeralStore;
use fastlog::RootFastlog;
use fbinit::FacebookInit;
use filenodes::ArcFilenodes;
use filestore::ArcFilestoreConfig;
use filestore::FilestoreConfig;
use fsnodes::RootFsnodeId;
use git_types::TreeHandle;
use hooks::ArcHookManager;
use hooks::HookManager;
use hooks_content_stores::RepoFileContentManager;
use live_commit_sync_config::TestLiveCommitSyncConfig;
use maplit::hashmap;
use maplit::hashset;
use megarepo_mapping::MegarepoMapping;
use memblob::Memblob;
use mercurial_derived_data::MappedHgChangesetId;
use mercurial_mutation::ArcHgMutationStore;
use mercurial_mutation::SqlHgMutationStoreBuilder;
use metaconfig_types::ArcRepoConfig;
use metaconfig_types::BlameVersion;
use metaconfig_types::DerivedDataConfig;
use metaconfig_types::DerivedDataTypesConfig;
use metaconfig_types::RepoConfig;
use metaconfig_types::SegmentedChangelogConfig;
use metaconfig_types::SegmentedChangelogHeadConfig;
use metaconfig_types::UnodeVersion;
use mononoke_types::RepositoryId;
use mutable_counters::ArcMutableCounters;
use mutable_counters::SqlMutableCountersBuilder;
use mutable_renames::ArcMutableRenames;
use mutable_renames::MutableRenames;
use mutable_renames::SqlMutableRenamesStore;
use newfilenodes::NewFilenodesBuilder;
use phases::ArcPhases;
use pushrebase_mutation_mapping::ArcPushrebaseMutationMapping;
use pushrebase_mutation_mapping::SqlPushrebaseMutationMappingConnection;
use redactedblobstore::RedactedBlobs;
use rendezvous::RendezVousOptions;
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
use repo_lock::AlwaysUnlockedRepoLock;
use repo_lock::ArcRepoLock;
use repo_lock::SqlRepoLock;
use repo_permission_checker::AlwaysAllowRepoPermissionChecker;
use repo_permission_checker::ArcRepoPermissionChecker;
use repo_sparse_profiles::ArcRepoSparseProfiles;
use repo_sparse_profiles::RepoSparseProfiles;
use repo_sparse_profiles::SqlSparseProfilesSizes;
use requests_table::SqlLongRunningRequestsQueue;
use scuba_ext::MononokeScubaSampleBuilder;
use segmented_changelog::new_test_segmented_changelog;
use segmented_changelog::SegmentedChangelogSqlConnections;
use segmented_changelog_types::ArcSegmentedChangelog;
use skeleton_manifest::RootSkeletonManifestId;
use skiplist::ArcSkiplistIndex;
use skiplist::SkiplistIndex;
use sql::rusqlite::Connection as SqliteConnection;
use sql::Connection;
use sql::SqlConnections;
use sql::SqlConnectionsWithSchema;
use sql_construct::SqlConstruct;
use sql_query_config::ArcSqlQueryConfig;
use sql_query_config::SqlQueryConfig;
use sqlphases::SqlPhasesBuilder;
use streaming_clone::ArcStreamingClone;
use streaming_clone::StreamingCloneBuilder;
use synced_commit_mapping::ArcSyncedCommitMapping;
use synced_commit_mapping::SqlSyncedCommitMapping;
use unodes::RootUnodeManifestId;
use wireproto_handler::ArcRepoHandlerBase;
use wireproto_handler::PushRedirectorBase;
use wireproto_handler::RepoHandlerBase;
use wireproto_handler::TargetRepoDbs;

/// Factory to construct test repositories.
///
/// This factory acts as a long-lived builder which can produce multiple
/// repositories with shared back-end storage or based on similar or the same
/// config.
///
/// By default, it will use a single in-memory blobstore and a single
/// in-memory metadata database for all repositories.
pub struct TestRepoFactory {
    /// Sometimes needed to construct a facet
    pub fb: FacebookInit,
    name: String,
    config: RepoConfig,
    blobstore: Arc<dyn Blobstore>,
    metadata_db: SqlConnectionsWithSchema,
    hg_mutation_db: SqlConnectionsWithSchema,
    redacted: Option<Arc<RedactedBlobs>>,
    permission_checker: Option<ArcRepoPermissionChecker>,
    derived_data_lease: Option<Box<dyn Fn() -> Arc<dyn LeaseOps> + Send + Sync>>,
    filenodes_override: Option<Box<dyn Fn(ArcFilenodes) -> ArcFilenodes + Send + Sync>>,
}

/// The default configuration for test repositories.
///
/// This configuration enables all derived data types at the latest version.
pub fn default_test_repo_config() -> RepoConfig {
    let derived_data_types_config = DerivedDataTypesConfig {
        types: hashset! {
            BlameRoot::NAME.to_string(),
            FilenodesOnlyPublic::NAME.to_string(),
            ChangesetInfo::NAME.to_string(),
            RootFastlog::NAME.to_string(),
            RootFsnodeId::NAME.to_string(),
            RootDeletedManifestV2Id::NAME.to_string(),
            RootUnodeManifestId::NAME.to_string(),
            TreeHandle::NAME.to_string(),
            MappedHgChangesetId::NAME.to_string(),
            RootSkeletonManifestId::NAME.to_string(),
            RootBasenameSuffixSkeletonManifest::NAME.to_string(),
        },
        unode_version: UnodeVersion::V2,
        blame_version: BlameVersion::V2,
        ..Default::default()
    };
    RepoConfig {
        derived_data_config: DerivedDataConfig {
            scuba_table: None,
            enabled_config_name: "default".to_string(),
            available_configs: hashmap![
                "default".to_string() => derived_data_types_config.clone(),
                "backfilling".to_string() => derived_data_types_config
            ],
        },
        segmented_changelog_config: SegmentedChangelogConfig {
            enabled: true,
            heads_to_include: vec![SegmentedChangelogHeadConfig::Bookmark(
                BookmarkName::new("master").unwrap(),
            )],
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Create an empty in-memory repo for tests.
///
/// This covers the simplest case.  For more complicated repositories, use
/// `TestRepoFactory`.
pub fn build_empty<R>(fb: FacebookInit) -> Result<R>
where
    R: for<'builder> facet::Buildable<TestRepoFactoryBuilder<'builder>>,
{
    Ok(TestRepoFactory::new(fb)?.build()?)
}

impl TestRepoFactory {
    /// Create a new factory for test repositories with default settings.
    pub fn new(fb: FacebookInit) -> Result<TestRepoFactory> {
        Self::with_sqlite_connection(
            fb,
            SqliteConnection::open_in_memory()?,
            SqliteConnection::open_in_memory()?,
        )
    }

    /// Create a new factory for test repositories with an existing Sqlite
    /// connection.
    pub fn with_sqlite_connection(
        fb: FacebookInit,
        metadata_con: SqliteConnection,
        hg_mutation_con: SqliteConnection,
    ) -> Result<TestRepoFactory> {
        metadata_con.execute_batch(MegarepoMapping::CREATION_QUERY)?;
        metadata_con.execute_batch(SqlMutableCountersBuilder::CREATION_QUERY)?;
        metadata_con.execute_batch(SqlBookmarksBuilder::CREATION_QUERY)?;
        metadata_con.execute_batch(SqlChangesetsBuilder::CREATION_QUERY)?;
        metadata_con.execute_batch(SqlBonsaiGitMappingBuilder::CREATION_QUERY)?;
        metadata_con.execute_batch(SqlBonsaiGlobalrevMappingBuilder::CREATION_QUERY)?;
        metadata_con.execute_batch(SqlBonsaiSvnrevMappingBuilder::CREATION_QUERY)?;
        metadata_con.execute_batch(SqlBonsaiHgMappingBuilder::CREATION_QUERY)?;
        metadata_con.execute_batch(SqlPhasesBuilder::CREATION_QUERY)?;
        metadata_con.execute_batch(SqlPushrebaseMutationMappingConnection::CREATION_QUERY)?;
        metadata_con.execute_batch(SqlLongRunningRequestsQueue::CREATION_QUERY)?;
        metadata_con.execute_batch(SqlMutableRenamesStore::CREATION_QUERY)?;
        metadata_con.execute_batch(SqlSyncedCommitMapping::CREATION_QUERY)?;
        metadata_con.execute_batch(SegmentedChangelogSqlConnections::CREATION_QUERY)?;
        metadata_con.execute_batch(SqlRepoLock::CREATION_QUERY)?;
        metadata_con.execute_batch(SqlSparseProfilesSizes::CREATION_QUERY)?;
        metadata_con.execute_batch(StreamingCloneBuilder::CREATION_QUERY)?;
        let metadata_db =
            SqlConnectionsWithSchema::new_single(Connection::with_sqlite(metadata_con));

        hg_mutation_con.execute_batch(SqlHgMutationStoreBuilder::CREATION_QUERY)?;
        let hg_mutation_db =
            SqlConnectionsWithSchema::new_single(Connection::with_sqlite(hg_mutation_con));

        Ok(TestRepoFactory {
            fb,
            name: "repo".to_string(),
            config: default_test_repo_config(),
            blobstore: Arc::new(Memblob::default()),
            metadata_db,
            hg_mutation_db,
            redacted: None,
            permission_checker: None,
            derived_data_lease: None,
            filenodes_override: None,
        })
    }

    /// Get the metadata database this factory is using for repositories.
    pub fn metadata_db(&self) -> &SqlConnectionsWithSchema {
        &self.metadata_db
    }

    /// Set the name for the next repository being built.
    pub fn with_name(&mut self, name: impl Into<String>) -> &mut Self {
        self.name = name.into();
        self
    }

    /// Set the ID for the next repository being built.
    pub fn with_id(&mut self, id: RepositoryId) -> &mut Self {
        self.config.repoid = id;
        self
    }

    /// Use a particular blobstore for repos built by this factory.
    pub fn with_blobstore(&mut self, blobstore: Arc<dyn Blobstore>) -> &mut Self {
        self.blobstore = blobstore;
        self
    }

    /// Redact content in repos that are built by this factory.
    pub fn redacted(&mut self, redacted: Option<RedactedBlobs>) -> &mut Self {
        self.redacted = redacted.map(Arc::new);
        self
    }

    /// Set a custom permission checker
    pub fn with_permission_checker(
        &mut self,
        permission_checker: ArcRepoPermissionChecker,
    ) -> &mut Self {
        self.permission_checker = Some(permission_checker);
        self
    }

    /// Modify the config of the repo.
    pub fn with_config_override(&mut self, modify: impl FnOnce(&mut RepoConfig)) -> &mut Self {
        modify(&mut self.config);
        self
    }

    /// Override the constructor for the derived data lease.
    pub fn with_derived_data_lease(
        &mut self,
        lease: impl Fn() -> Arc<dyn LeaseOps> + Send + Sync + 'static,
    ) -> &mut Self {
        self.derived_data_lease = Some(Box::new(lease));
        self
    }

    /// Override filenodes.
    pub fn with_filenodes_override(
        &mut self,
        filenodes_override: impl Fn(ArcFilenodes) -> ArcFilenodes + Send + Sync + 'static,
    ) -> &mut Self {
        self.filenodes_override = Some(Box::new(filenodes_override));
        self
    }

    /// Function to create megarepo mapping from the same connection as other DBs
    pub fn megarepo_mapping(&self) -> Arc<MegarepoMapping> {
        Arc::new(MegarepoMapping::from_sql_connections(
            self.metadata_db.clone().into(),
        ))
    }
}

#[facet::factory()]
impl TestRepoFactory {
    /// Construct RepoConfig based on the config in the factory.
    pub fn repo_config(&self) -> ArcRepoConfig {
        Arc::new(self.config.clone())
    }

    /// Construct RepoIdentity based on the config and name in the factory.
    pub fn repo_identity(&self, repo_config: &ArcRepoConfig) -> ArcRepoIdentity {
        Arc::new(RepoIdentity::new(repo_config.repoid, self.name.clone()))
    }

    /// Construct Changesets using the in-memory metadata database.
    pub fn changesets(&self, repo_identity: &ArcRepoIdentity) -> Result<ArcChangesets> {
        Ok(Arc::new(
            SqlChangesetsBuilder::from_sql_connections(self.metadata_db.clone().into())
                .build(RendezVousOptions::for_test(), repo_identity.id()),
        ))
    }

    /// Construct a Changeset Fetcher.
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

    /// Construct SQL bookmarks using the in-memory metadata database.
    pub fn sql_bookmarks(&self, repo_identity: &ArcRepoIdentity) -> Result<ArcSqlBookmarks> {
        Ok(Arc::new(
            SqlBookmarksBuilder::from_sql_connections(self.metadata_db.clone().into())
                .with_repo_id(repo_identity.id()),
        ))
    }

    /// Construct Bookmarks.
    pub fn bookmarks(&self, sql_bookmarks: &ArcSqlBookmarks) -> ArcBookmarks {
        sql_bookmarks.clone()
    }

    /// Construct Bookmark update log.
    pub fn bookmark_update_log(&self, sql_bookmarks: &ArcSqlBookmarks) -> ArcBookmarkUpdateLog {
        sql_bookmarks.clone()
    }

    /// Construct Phases.
    pub fn phases(
        &self,
        repo_identity: &ArcRepoIdentity,
        bookmarks: &ArcBookmarks,
        changeset_fetcher: &ArcChangesetFetcher,
    ) -> ArcPhases {
        let sql_phases_builder =
            SqlPhasesBuilder::from_sql_connections(self.metadata_db.clone().into());
        let heads_fetcher = bookmark_heads_fetcher(bookmarks.clone());
        sql_phases_builder.build(repo_identity.id(), changeset_fetcher.clone(), heads_fetcher)
    }

    /// Construct Bonsai Hg Mapping using the in-memory metadata database.
    pub fn bonsai_hg_mapping(&self, repo_identity: &ArcRepoIdentity) -> Result<ArcBonsaiHgMapping> {
        Ok(Arc::new(
            SqlBonsaiHgMappingBuilder::from_sql_connections(self.metadata_db.clone().into())
                .build(repo_identity.id(), RendezVousOptions::for_test()),
        ))
    }

    /// Construct Bonsai Git Mapping using the in-memory metadata database.
    pub fn bonsai_git_mapping(
        &self,
        repo_identity: &ArcRepoIdentity,
    ) -> Result<ArcBonsaiGitMapping> {
        Ok(Arc::new(
            SqlBonsaiGitMappingBuilder::from_sql_connections(self.metadata_db.clone().into())
                .build(repo_identity.id()),
        ))
    }

    /// Construct Bonsai Globalrev Mapping using the in-memory metadata
    /// database.
    pub fn bonsai_globalrev_mapping(
        &self,
        repo_identity: &ArcRepoIdentity,
    ) -> Result<ArcBonsaiGlobalrevMapping> {
        Ok(Arc::new(
            SqlBonsaiGlobalrevMappingBuilder::from_sql_connections(self.metadata_db.clone().into())
                .build(repo_identity.id()),
        ))
    }

    /// Construct Bonsai Svnrev Mapping using the in-memory metadata
    /// database.
    pub fn bonsai_svnrev_mapping(
        &self,
        repo_identity: &ArcRepoIdentity,
    ) -> Result<ArcBonsaiSvnrevMapping> {
        Ok(Arc::new(
            SqlBonsaiSvnrevMappingBuilder::from_sql_connections(self.metadata_db.clone().into())
                .build(repo_identity.id()),
        ))
    }

    /// Construct Pushrebase Mutation Mapping using the in-memory metadata
    /// database.
    pub fn pushrebase_mutation_mapping(
        &self,
        repo_identity: &ArcRepoIdentity,
    ) -> Result<ArcPushrebaseMutationMapping> {
        Ok(Arc::new(
            SqlPushrebaseMutationMappingConnection::from_sql_connections(
                self.metadata_db.clone().into(),
            )
            .with_repo_id(repo_identity.id()),
        ))
    }

    /// Construct permission checker.  By default this allows all access.
    pub fn permission_checker(&self) -> Result<ArcRepoPermissionChecker> {
        if let Some(permission_checker) = &self.permission_checker {
            Ok(permission_checker.clone())
        } else {
            let permission_checker = AlwaysAllowRepoPermissionChecker::new();
            Ok(Arc::new(permission_checker))
        }
    }

    /// Construct Filenodes.
    ///
    /// Filenodes do not use the metadata db (each repo has its own filenodes
    /// db in memory).
    pub fn filenodes(&self, repo_identity: &ArcRepoIdentity) -> Result<ArcFilenodes> {
        let mut filenodes: ArcFilenodes =
            Arc::new(NewFilenodesBuilder::with_sqlite_in_memory()?.build(repo_identity.id()));
        if let Some(filenodes_override) = &self.filenodes_override {
            filenodes = filenodes_override(filenodes);
        }
        Ok(filenodes)
    }

    /// Construct Mercurial Mutation Store using the in-memory hg_mutation
    /// database.
    pub fn hg_mutation_store(&self, repo_identity: &ArcRepoIdentity) -> Result<ArcHgMutationStore> {
        Ok(Arc::new(
            SqlHgMutationStoreBuilder::from_sql_connections(self.hg_mutation_db.clone().into())
                .with_repo_id(repo_identity.id()),
        ))
    }

    /// Construct Segmented Changelog.  Segmented changelog is disabled in
    /// test repos.
    pub fn segmented_changelog(
        &self,
        repo_identity: &ArcRepoIdentity,
        repo_config: &ArcRepoConfig,
        changeset_fetcher: &ArcChangesetFetcher,
        bookmarks: &ArcBookmarks,
    ) -> Result<ArcSegmentedChangelog> {
        new_test_segmented_changelog(
            CoreContext::test_mock(self.fb),
            repo_identity.id(),
            &repo_config.segmented_changelog_config,
            changeset_fetcher.clone(),
            bookmarks.clone(),
        )
    }

    /// Construct RepoDerivedData.  Derived data uses an in-process lease for
    /// tests.
    pub fn repo_derived_data(
        &self,
        repo_identity: &ArcRepoIdentity,
        repo_config: &ArcRepoConfig,
        changesets: &ArcChangesets,
        bonsai_hg_mapping: &ArcBonsaiHgMapping,
        filenodes: &ArcFilenodes,
        repo_blobstore: &ArcRepoBlobstore,
    ) -> Result<ArcRepoDerivedData> {
        let lease = self.derived_data_lease.as_ref().map_or_else(
            || Arc::new(InProcessLease::new()) as Arc<dyn LeaseOps>,
            |lease| lease(),
        );
        Ok(Arc::new(RepoDerivedData::new(
            repo_identity.id(),
            repo_identity.name().to_string(),
            changesets.clone(),
            bonsai_hg_mapping.clone(),
            filenodes.clone(),
            repo_blobstore.as_ref().clone(),
            lease,
            MononokeScubaSampleBuilder::with_discard(),
            repo_config.derived_data_config.clone(),
            None, // derivation_service_client = None
        )?))
    }

    /// Construct the RepoBlobstore using the blobstore in the factory.
    pub fn repo_blobstore(&self, repo_identity: &ArcRepoIdentity) -> ArcRepoBlobstore {
        let repo_blobstore = RepoBlobstore::new(
            self.blobstore.clone(),
            self.redacted.clone(),
            repo_identity.id(),
            MononokeScubaSampleBuilder::with_discard(),
        );
        Arc::new(repo_blobstore)
    }

    /// Construct filestore config based on the config in the factory.
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

    /// Create empty skiplist index
    pub fn skiplist_index(&self) -> ArcSkiplistIndex {
        Arc::new(SkiplistIndex::new())
    }

    /// Disabled ephemeral repo
    pub fn repo_ephemeral_store(&self, repo_identity: &ArcRepoIdentity) -> ArcRepoEphemeralStore {
        Arc::new(RepoEphemeralStore::disabled(repo_identity.id()))
    }

    /// Mutable renames
    pub fn mutable_renames(&self, repo_identity: &ArcRepoIdentity) -> Result<ArcMutableRenames> {
        let sql_store =
            SqlMutableRenamesStore::from_sql_connections(self.metadata_db.clone().into());
        Ok(Arc::new(MutableRenames::new_test(
            repo_identity.id(),
            sql_store,
        )))
    }

    /// The commit mapping bettween repos for synced commits.
    pub fn synced_commit_mapping(&self) -> Result<ArcSyncedCommitMapping> {
        Ok(Arc::new(SqlSyncedCommitMapping::from_sql_connections(
            self.metadata_db.clone().into(),
        )))
    }

    /// Cross-repo sync manager for this repo
    pub fn repo_cross_repo(
        &self,
        synced_commit_mapping: &ArcSyncedCommitMapping,
    ) -> Result<ArcRepoCrossRepo> {
        let live_commit_sync_config = Arc::new(TestLiveCommitSyncConfig::new_empty());
        let sync_lease = Arc::new(InProcessLease::new());
        Ok(Arc::new(RepoCrossRepo::new(
            synced_commit_mapping.clone(),
            live_commit_sync_config,
            sync_lease,
        )))
    }

    /// Test repo-handler-base
    pub fn repo_handler_base(
        &self,
        repo_config: &ArcRepoConfig,
        repo_cross_repo: &ArcRepoCrossRepo,
        repo_identity: &ArcRepoIdentity,
        bookmarks: &ArcBookmarks,
        bookmark_update_log: &ArcBookmarkUpdateLog,
        mutable_counters: &ArcMutableCounters,
    ) -> Result<ArcRepoHandlerBase> {
        let ctx = CoreContext::test_mock(self.fb);
        let scuba = ctx.scuba().clone();
        let logger = ctx.logger().clone();
        let repo_client_knobs = repo_config.repo_client_knobs.clone();

        let common_commit_sync_config = repo_cross_repo
            .live_commit_sync_config()
            .clone()
            .get_common_config_if_exists(repo_identity.id())?;
        let synced_commit_mapping = repo_cross_repo.synced_commit_mapping();
        let backup_repo_config = repo_config.backup_repo_config.clone();
        let target_repo_dbs = Arc::new(TargetRepoDbs {
            connections: SqlConnections::from(self.metadata_db.clone()),
            bookmarks: bookmarks.clone(),
            bookmark_update_log: bookmark_update_log.clone(),
            counters: mutable_counters.clone(),
        });

        let maybe_push_redirector_base =
            common_commit_sync_config.map(|common_commit_sync_config| {
                Arc::new(PushRedirectorBase {
                    common_commit_sync_config,
                    target_repo_dbs,
                    synced_commit_mapping: synced_commit_mapping.clone(),
                })
            });
        Ok(Arc::new(RepoHandlerBase {
            logger,
            scuba,
            repo_client_knobs,
            maybe_push_redirector_base,
            backup_repo_config,
        }))
    }

    /// Mutable counters
    pub fn mutable_counters(&self, repo_identity: &ArcRepoIdentity) -> Result<ArcMutableCounters> {
        Ok(Arc::new(
            SqlMutableCountersBuilder::from_sql_connections(self.metadata_db.clone().into())
                .build(repo_identity.id()),
        ))
    }

    /// Set of DerivedDataManagers for DDS
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
        let lease = self.derived_data_lease.as_ref().map_or_else(
            || Arc::new(InProcessLease::new()) as Arc<dyn LeaseOps>,
            |lease| lease(),
        );
        let ctx = CoreContext::test_mock(self.fb);
        let logger = ctx.logger().clone();
        anyhow::Ok(Arc::new(DerivedDataManagerSet::new(
            repo_identity.id(),
            repo_identity.name().to_string(),
            changesets.clone(),
            bonsai_hg_mapping.clone(),
            filenodes.clone(),
            repo_blobstore.as_ref().clone(),
            lease,
            logger,
            MononokeScubaSampleBuilder::with_discard(),
            config,
            None, // derivation_service_client = None
        )?))
    }

    /// ACL regions
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

    /// Hook manager
    pub fn hook_manager(
        &self,
        repo_identity: &ArcRepoIdentity,
        repo_derived_data: &ArcRepoDerivedData,
        bookmarks: &ArcBookmarks,
        repo_blobstore: &ArcRepoBlobstore,
    ) -> ArcHookManager {
        let content_store = RepoFileContentManager::from_parts(
            bookmarks.clone(),
            repo_blobstore.clone(),
            repo_derived_data.clone(),
        );

        Arc::new(HookManager::new_test(
            repo_identity.name().to_string(),
            Box::new(content_store),
        ))
    }

    /// Sparse profiles
    pub fn sparse_profile(&self, _repo_config: &ArcRepoConfig) -> ArcRepoSparseProfiles {
        Arc::new(RepoSparseProfiles {
            sql_profile_sizes: None,
        })
    }

    /// Construct unlocked repo lock.
    pub fn repo_lock(&self, repo_identity: &ArcRepoIdentity) -> Result<ArcRepoLock> {
        let repo_lock = AlwaysUnlockedRepoLock::new(repo_identity.id());
        Ok(Arc::new(repo_lock))
    }

    /// Repo bookmark attrs
    pub fn repo_bookmark_attrs(&self, repo_config: &ArcRepoConfig) -> Result<ArcRepoBookmarkAttrs> {
        Ok(Arc::new(RepoBookmarkAttrs::new_test(
            repo_config.bookmarks.clone(),
        )?))
    }

    /// Streaming clone
    pub fn streaming_clone(
        &self,
        repo_identity: &ArcRepoIdentity,
        repo_blobstore: &ArcRepoBlobstore,
    ) -> ArcStreamingClone {
        Arc::new(
            StreamingCloneBuilder::from_sql_connections(self.metadata_db.clone().into())
                .build(repo_identity.id(), repo_blobstore.clone()),
        )
    }

    /// Sql query config
    pub fn sql_query_config(&self) -> ArcSqlQueryConfig {
        Arc::new(SqlQueryConfig { caching: None })
    }
}
