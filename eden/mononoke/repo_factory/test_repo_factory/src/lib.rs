/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Repository factory for tests.
#![deny(missing_docs)]

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use blame::BlameRoot;
use blobstore::Blobstore;
use bonsai_git_mapping::{ArcBonsaiGitMapping, SqlBonsaiGitMappingConnection};
use bonsai_globalrev_mapping::{ArcBonsaiGlobalrevMapping, SqlBonsaiGlobalrevMapping};
use bonsai_hg_mapping::{ArcBonsaiHgMapping, SqlBonsaiHgMappingBuilder};
use bonsai_svnrev_mapping::{
    ArcRepoBonsaiSvnrevMapping, RepoBonsaiSvnrevMapping, SqlBonsaiSvnrevMapping,
};
use bookmarks::{ArcBookmarkUpdateLog, ArcBookmarks};
use cacheblob::{InProcessLease, LeaseOps};
use changeset_fetcher::{ArcChangesetFetcher, SimpleChangesetFetcher};
use changeset_info::ChangesetInfo;
use changesets::{ArcChangesets, SqlChangesets};
use dbbookmarks::{ArcSqlBookmarks, SqlBookmarksBuilder};
use deleted_files_manifest::RootDeletedManifestId;
use derived_data::BonsaiDerivable;
use derived_data_filenodes::FilenodesOnlyPublic;
use fastlog::RootFastlog;
use filenodes::ArcFilenodes;
use filestore::{ArcFilestoreConfig, FilestoreConfig};
use fsnodes::RootFsnodeId;
use git_types::TreeHandle;
use maplit::hashset;
use memblob::Memblob;
use mercurial_derived_data::MappedHgChangesetId;
use mercurial_mutation::{ArcHgMutationStore, SqlHgMutationStoreBuilder};
use metaconfig_types::{
    ArcRepoConfig, DerivedDataConfig, DerivedDataTypesConfig, RepoConfig, UnodeVersion,
};
use mononoke_types::RepositoryId;
use mutable_counters::SqlMutableCounters;
use newfilenodes::NewFilenodesBuilder;
use phases::{ArcSqlPhasesFactory, SqlPhasesFactory};
use redactedblobstore::RedactedMetadata;
use repo_blobstore::{ArcRepoBlobstore, RepoBlobstoreArgs};
use repo_derived_data::{ArcRepoDerivedData, RepoDerivedData};
use repo_identity::{ArcRepoIdentity, RepoIdentity};
use scuba_ext::MononokeScubaSampleBuilder;
use segmented_changelog::DisabledSegmentedChangelog;
use segmented_changelog_types::ArcSegmentedChangelog;
use skeleton_manifest::RootSkeletonManifestId;
use sql::{rusqlite::Connection as SqliteConnection, Connection};
use sql_construct::SqlConstruct;
use sql_ext::SqlConnections;
use unodes::RootUnodeManifestId;

/// Factory to construct test repositories.
///
/// This factory acts as a long-lived builder which can produce multiple
/// repositories with shared back-end storage or based on similar or the same
/// config.
///
/// By default, it will use a single in-memory blobstore and a single
/// in-memory metadata database for all repositories.
pub struct TestRepoFactory {
    name: String,
    config: RepoConfig,
    blobstore: Arc<dyn Blobstore>,
    metadata_db: SqlConnections,
    redacted: Option<HashMap<String, RedactedMetadata>>,
    derived_data_lease: Option<Box<dyn Fn() -> Arc<dyn LeaseOps> + Send + Sync>>,
    filenodes_override: Option<Box<dyn Fn(ArcFilenodes) -> ArcFilenodes + Send + Sync>>,
}

fn default_test_repo_config() -> RepoConfig {
    RepoConfig {
        derived_data_config: DerivedDataConfig {
            scuba_table: None,
            enabled: DerivedDataTypesConfig {
                types: hashset! {
                    BlameRoot::NAME.to_string(),
                    FilenodesOnlyPublic::NAME.to_string(),
                    ChangesetInfo::NAME.to_string(),
                    RootFastlog::NAME.to_string(),
                    RootFsnodeId::NAME.to_string(),
                    RootSkeletonManifestId::NAME.to_string(),
                    RootDeletedManifestId::NAME.to_string(),
                    RootUnodeManifestId::NAME.to_string(),
                    TreeHandle::NAME.to_string(),
                    MappedHgChangesetId::NAME.to_string(),
                },
                unode_version: UnodeVersion::V2,
                ..Default::default()
            },
            backfilling: DerivedDataTypesConfig::default(),
        },
        ..Default::default()
    }
}

/// Create an empty in-memory repo for tests.
///
/// This covers the simplest case.  For more complicated repositories, use
/// `TestRepoFactory`.
pub fn build_empty<R>() -> Result<R>
where
    R: for<'builder> facet::Buildable<TestRepoFactoryBuilder<'builder>>,
{
    Ok(TestRepoFactory::new()?.build()?)
}

impl TestRepoFactory {
    /// Create a new factory for test repositories with default settings.
    pub fn new() -> Result<TestRepoFactory> {
        let con = SqliteConnection::open_in_memory()?;
        Self::with_sqlite_connection(con)
    }

    /// Create a new factory for test repositories with an existing Sqlite
    /// connection.
    pub fn with_sqlite_connection(con: SqliteConnection) -> Result<TestRepoFactory> {
        con.execute_batch(SqlMutableCounters::CREATION_QUERY)?;
        con.execute_batch(SqlBookmarksBuilder::CREATION_QUERY)?;
        con.execute_batch(SqlChangesets::CREATION_QUERY)?;
        con.execute_batch(SqlBonsaiGitMappingConnection::CREATION_QUERY)?;
        con.execute_batch(SqlBonsaiGlobalrevMapping::CREATION_QUERY)?;
        con.execute_batch(SqlBonsaiSvnrevMapping::CREATION_QUERY)?;
        con.execute_batch(SqlBonsaiHgMappingBuilder::CREATION_QUERY)?;
        con.execute_batch(SqlPhasesFactory::CREATION_QUERY)?;
        con.execute_batch(SqlHgMutationStoreBuilder::CREATION_QUERY)?;
        let metadata_db = SqlConnections::new_single(Connection::with_sqlite(con));

        Ok(TestRepoFactory {
            name: "repo".to_string(),
            config: default_test_repo_config(),
            blobstore: Arc::new(Memblob::default()),
            metadata_db,
            redacted: None,
            derived_data_lease: None,
            filenodes_override: None,
        })
    }

    /// Get the metadata database this factory is using for repositories.
    pub fn metadata_db(&self) -> &SqlConnections {
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
    pub fn redacted(&mut self, redacted: Option<HashMap<String, RedactedMetadata>>) -> &mut Self {
        self.redacted = redacted;
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
    pub fn changesets(&self) -> Result<ArcChangesets> {
        Ok(Arc::new(SqlChangesets::from_sql_connections(
            self.metadata_db.clone(),
        )))
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
            SqlBookmarksBuilder::from_sql_connections(self.metadata_db.clone())
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

    /// Construct a SQL Phases Factory.
    pub fn sql_phases_factory(&self) -> Result<ArcSqlPhasesFactory> {
        // TODO(mbthomas) we should be constructing Arc<Phases> directly.
        Ok(Arc::new(SqlPhasesFactory::from_sql_connections(
            self.metadata_db.clone(),
        )))
    }

    /// Construct Bonsai Hg Mapping using the in-memory metadata database.
    pub fn bonsai_hg_mapping(&self) -> Result<ArcBonsaiHgMapping> {
        Ok(Arc::new(
            SqlBonsaiHgMappingBuilder::from_sql_connections(self.metadata_db.clone()).build(),
        ))
    }

    /// Construct Bonsai Git Mapping using the in-memory metadata database.
    pub fn bonsai_git_mapping(
        &self,
        repo_identity: &ArcRepoIdentity,
    ) -> Result<ArcBonsaiGitMapping> {
        Ok(Arc::new(
            SqlBonsaiGitMappingConnection::from_sql_connections(self.metadata_db.clone())
                .with_repo_id(repo_identity.id()),
        ))
    }

    /// Construct Bonsai Globalrev Mapping using the in-memory metadata
    /// database.
    pub fn bonsai_globalrev_mapping(&self) -> Result<ArcBonsaiGlobalrevMapping> {
        Ok(Arc::new(SqlBonsaiGlobalrevMapping::from_sql_connections(
            self.metadata_db.clone(),
        )))
    }

    /// Construct Repo Bonsai Svnrev Mapping using the in-memory metadata
    /// database.
    pub fn repo_bonsai_svnrev_mapping(
        &self,
        repo_identity: &ArcRepoIdentity,
    ) -> Result<ArcRepoBonsaiSvnrevMapping> {
        Ok(Arc::new(RepoBonsaiSvnrevMapping::new(
            repo_identity.id(),
            Arc::new(SqlBonsaiSvnrevMapping::from_sql_connections(
                self.metadata_db.clone(),
            )),
        )))
    }

    /// Construct Filenodes.
    ///
    /// Filenodes do not use the metadata db (each repo has its own filenodes
    /// db in memory).
    pub fn filenodes(&self) -> Result<ArcFilenodes> {
        let mut filenodes: ArcFilenodes =
            Arc::new(NewFilenodesBuilder::with_sqlite_in_memory()?.build());
        if let Some(filenodes_override) = &self.filenodes_override {
            filenodes = filenodes_override(filenodes);
        }
        Ok(filenodes)
    }

    /// Construct Mercurial Mutation Store using the in-memory metadata
    /// database.
    pub fn hg_mutation_store(&self, repo_identity: &ArcRepoIdentity) -> Result<ArcHgMutationStore> {
        Ok(Arc::new(
            SqlHgMutationStoreBuilder::from_sql_connections(self.metadata_db.clone())
                .with_repo_id(repo_identity.id()),
        ))
    }

    /// Construct Segmented Changelog.  Segmented changelog is disabled in
    /// test repos.
    pub fn segmented_changelog(&self) -> ArcSegmentedChangelog {
        Arc::new(DisabledSegmentedChangelog::new())
    }

    /// Construct RepoDerivedData.  Derived data uses an in-process lease for
    /// tests.
    pub fn repo_derived_data(&self, repo_config: &ArcRepoConfig) -> ArcRepoDerivedData {
        let lease = self.derived_data_lease.as_ref().map_or_else(
            || Arc::new(InProcessLease::new()) as Arc<dyn LeaseOps>,
            |lease| lease(),
        );
        Arc::new(RepoDerivedData::new(
            repo_config.derived_data_config.clone(),
            lease,
        ))
    }

    /// Construct the RepoBlobstore using the blobstore in the factory.
    pub fn repo_blobstore(&self, repo_identity: &ArcRepoIdentity) -> ArcRepoBlobstore {
        let args = RepoBlobstoreArgs::new(
            self.blobstore.clone(),
            self.redacted.clone(),
            repo_identity.id(),
            MononokeScubaSampleBuilder::with_discard(),
        );
        let (repo_blobstore, _) = args.into_blobrepo_parts();
        Arc::new(repo_blobstore)
    }

    /// Construct filestore config based on the config in the factory.
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
