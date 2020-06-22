/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Context, Error, Result};
use blame::BlameRoot;
use blobrepo::BlobRepo;
use blobrepo_errors::*;
use blobstore::Blobstore;
use blobstore_factory::{make_blobstore, make_metadata_sql_factory, MetadataSqlFactory};
use bonsai_git_mapping::{BonsaiGitMapping, SqlBonsaiGitMappingConnection};
use bonsai_globalrev_mapping::{BonsaiGlobalrevMapping, SqlBonsaiGlobalrevMapping};
use bonsai_hg_mapping::{BonsaiHgMapping, CachingBonsaiHgMapping, SqlBonsaiHgMapping};
use bookmarks::{Bookmarks, CachedBookmarks};
use cacheblob::{
    new_cachelib_blobstore_no_lease, new_memcache_blobstore, InProcessLease, LeaseOps, MemcacheOps,
};
use changeset_info::ChangesetInfo;
use changesets::{CachingChangesets, Changesets, SqlChangesets};
use dbbookmarks::SqlBookmarks;
use deleted_files_manifest::RootDeletedManifestId;
use derived_data::BonsaiDerived;
use derived_data_filenodes::FilenodesOnlyPublic;
use fastlog::RootFastlog;
use fbinit::FacebookInit;
use filenodes::Filenodes;
use filestore::FilestoreConfig;
use fsnodes::RootFsnodeId;
use futures::{compat::Future01CompatExt, future};
use futures_util::try_join;
use git_types::TreeHandle;
use maplit::btreeset;
use memblob::EagerMemblob;
use mercurial_mutation::{HgMutationStore, SqlHgMutationStoreBuilder};
use metaconfig_types::{
    self, DerivedDataConfig, FilestoreParams, Redaction, RepoConfig, StorageConfig, UnodeVersion,
};
use mononoke_types::RepositoryId;
use newfilenodes::NewFilenodesBuilder;
use phases::SqlPhasesFactory;
use readonlyblob::ReadOnlyBlobstore;
use redactedblobstore::SqlRedactedContentStore;
use repo_blobstore::RepoBlobstoreArgs;
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};
use slog::Logger;
use sql::{rusqlite::Connection as SqliteConnection, Connection};
use sql_construct::SqlConstruct;
use sql_ext::{facebook::MysqlOptions, SqlConnections};
use std::{collections::HashMap, sync::Arc, time::Duration};
use type_map::TypeMap;
use unodes::RootUnodeManifestId;

pub use blobstore_factory::{BlobstoreOptions, ReadOnlyStorage};

#[derive(Copy, Clone, PartialEq)]
pub enum Caching {
    Enabled,
    Disabled,
    CachelibOnlyBlobstore,
}

const BLOBSTORE_BLOBS_CACHE_POOL: &'static str = "blobstore-blobs";
const BLOBSTORE_PRESENCE_CACHE_POOL: &'static str = "blobstore-presence";

pub struct BlobrepoBuilder<'a> {
    fb: FacebookInit,
    reponame: String,
    storage_config: StorageConfig,
    repoid: RepositoryId,
    mysql_options: MysqlOptions,
    caching: Caching,
    bookmarks_cache_ttl: Option<Duration>,
    redaction: Redaction,
    scuba_censored_table: Option<String>,
    filestore_params: Option<FilestoreParams>,
    readonly_storage: ReadOnlyStorage,
    blobstore_options: BlobstoreOptions,
    logger: &'a Logger,
    derived_data_config: DerivedDataConfig,
}

impl<'a> BlobrepoBuilder<'a> {
    pub fn new(
        fb: FacebookInit,
        reponame: String,
        config: &RepoConfig,
        mysql_options: MysqlOptions,
        caching: Caching,
        scuba_censored_table: Option<String>,
        readonly_storage: ReadOnlyStorage,
        blobstore_options: BlobstoreOptions,
        logger: &'a Logger,
    ) -> Self {
        Self {
            fb,
            reponame,
            storage_config: config.storage_config.clone(),
            repoid: config.repoid,
            mysql_options,
            caching,
            bookmarks_cache_ttl: config.bookmarks_cache_ttl.clone(),
            redaction: config.redaction.clone(),
            scuba_censored_table,
            filestore_params: config.filestore.clone(),
            readonly_storage,
            blobstore_options,
            logger,
            derived_data_config: config.derived_data_config.clone(),
        }
    }

    pub fn set_redaction(&mut self, redaction: Redaction) {
        self.redaction = redaction;
    }

    /// remote (ie, MySQL), then it configures a full set of caches. Otherwise with local storage
    /// it's assumed to be a test configuration.
    ///
    /// The blobstore config is actually orthogonal to this, but it wouldn't make much sense to
    /// configure a local blobstore with a remote db, or vice versa. There's no error checking
    /// at this level (aside from disallowing a multiplexed blobstore with a local db).
    pub async fn build(self) -> Result<BlobRepo, Error> {
        let BlobrepoBuilder {
            fb,
            reponame,
            storage_config,
            repoid,
            mysql_options,
            caching,
            bookmarks_cache_ttl,
            redaction,
            scuba_censored_table,
            filestore_params,
            readonly_storage,
            blobstore_options,
            logger,
            derived_data_config,
        } = self;

        let sql_factory = make_metadata_sql_factory(
            fb,
            storage_config.metadata,
            mysql_options,
            readonly_storage,
            // FIXME: remove clone when make_metadata_sql_factory is async-await
            logger.clone(),
        )
        .compat();

        let blobstore = make_blobstore(
            fb,
            storage_config.blobstore,
            mysql_options,
            readonly_storage,
            &blobstore_options,
            &logger,
        );

        let (sql_factory, blobstore) = future::try_join(sql_factory, blobstore).await?;

        open_blobrepo_given_datasources(
            fb,
            blobstore,
            sql_factory,
            repoid,
            caching,
            bookmarks_cache_ttl,
            redaction,
            scuba_censored_table,
            filestore_params,
            readonly_storage,
            derived_data_config,
            reponame,
        )
        .await
    }
}

/// Expose for graph walker that has storage open already
pub async fn open_blobrepo_given_datasources(
    fb: FacebookInit,
    blobstore: Arc<dyn Blobstore>,
    sql_factory: MetadataSqlFactory,
    repoid: RepositoryId,
    caching: Caching,
    bookmarks_cache_ttl: Option<Duration>,
    redaction: Redaction,
    scuba_censored_table: Option<String>,
    filestore_params: Option<FilestoreParams>,
    readonly_storage: ReadOnlyStorage,
    derived_data_config: DerivedDataConfig,
    reponame: String,
) -> Result<BlobRepo, Error> {
    let redacted_blobs = match redaction {
        Redaction::Enabled => {
            let redacted_blobs = sql_factory
                .open::<SqlRedactedContentStore>()
                .compat()
                .await?
                .get_all_redacted_blobs()
                .compat()
                .await?;
            Some(redacted_blobs)
        }
        Redaction::Disabled => None,
    };

    let filestore_config = filestore_params
        .map(|params| {
            let FilestoreParams {
                chunk_size,
                concurrency,
            } = params;

            FilestoreConfig {
                chunk_size: Some(chunk_size),
                concurrency,
            }
        })
        .unwrap_or_default();

    let repo = match caching {
        Caching::Disabled | Caching::CachelibOnlyBlobstore => {
            let blobstore = if caching == Caching::CachelibOnlyBlobstore {
                // Use cachelib
                let blob_pool = get_cache_pool(BLOBSTORE_BLOBS_CACHE_POOL)?;
                let presence_pool = get_cache_pool(BLOBSTORE_PRESENCE_CACHE_POOL)?;

                Arc::new(new_cachelib_blobstore_no_lease(
                    blobstore,
                    Arc::new(blob_pool),
                    Arc::new(presence_pool),
                ))
            } else {
                blobstore
            };

            new_development(
                fb,
                &sql_factory,
                blobstore,
                redacted_blobs,
                scuba_censored_table,
                repoid,
                filestore_config,
                bookmarks_cache_ttl,
                derived_data_config,
                reponame,
            )
            .await?
        }
        Caching::Enabled => {
            new_production(
                fb,
                &sql_factory,
                blobstore,
                redacted_blobs,
                scuba_censored_table,
                repoid,
                bookmarks_cache_ttl,
                filestore_config,
                readonly_storage,
                derived_data_config,
                reponame,
            )
            .await?
        }
    };

    Ok(repo)
}

/// A helper to build test repositories.
pub struct TestRepoBuilder {
    repo_id: RepositoryId,
    blobstore: Arc<dyn Blobstore>,
    redacted: Option<HashMap<String, String>>,
}

impl TestRepoBuilder {
    pub fn new() -> Self {
        Self {
            repo_id: RepositoryId::new(0),
            blobstore: Arc::new(EagerMemblob::new()),
            redacted: None,
        }
    }

    pub fn id(mut self, repo_id: RepositoryId) -> Self {
        self.repo_id = repo_id;
        self
    }

    pub fn redacted(mut self, redacted: Option<HashMap<String, String>>) -> Self {
        self.redacted = redacted;
        self
    }

    pub fn blobstore(mut self, blobstore: Arc<dyn Blobstore>) -> Self {
        self.blobstore = blobstore;
        self
    }

    fn maybe_blobstore(self, maybe_blobstore: Option<Arc<dyn Blobstore>>) -> Self {
        if let Some(blobstore) = maybe_blobstore {
            return self.blobstore(blobstore);
        }
        self
    }

    pub fn build(self) -> Result<BlobRepo> {
        let Self {
            repo_id,
            blobstore,
            redacted,
        } = self;

        let repo_blobstore_args = RepoBlobstoreArgs::new(
            blobstore,
            redacted,
            repo_id,
            ScubaSampleBuilder::with_discard(),
        );

        let phases_factory = SqlPhasesFactory::with_sqlite_in_memory()?;

        Ok(blobrepo_new(
            Arc::new(SqlBookmarks::with_sqlite_in_memory()?),
            repo_blobstore_args,
            Arc::new(
                NewFilenodesBuilder::with_sqlite_in_memory()
                    .context(ErrorKind::StateOpen(StateOpenError::Filenodes))?
                    .build(),
            ),
            Arc::new(
                SqlChangesets::with_sqlite_in_memory()
                    .context(ErrorKind::StateOpen(StateOpenError::Changesets))?,
            ),
            Arc::new(
                SqlBonsaiGitMappingConnection::with_sqlite_in_memory()
                    .context(ErrorKind::StateOpen(StateOpenError::BonsaiGitMapping))?
                    .with_repo_id(repo_id),
            ),
            Arc::new(
                SqlBonsaiGlobalrevMapping::with_sqlite_in_memory()
                    .context(ErrorKind::StateOpen(StateOpenError::BonsaiGlobalrevMapping))?,
            ),
            Arc::new(
                SqlBonsaiHgMapping::with_sqlite_in_memory()
                    .context(ErrorKind::StateOpen(StateOpenError::BonsaiHgMapping))?,
            ),
            Arc::new(
                SqlHgMutationStoreBuilder::with_sqlite_in_memory()
                    .context(ErrorKind::StateOpen(StateOpenError::HgMutationStore))?
                    .with_repo_id(repo_id),
            ),
            Arc::new(InProcessLease::new()),
            FilestoreConfig::default(),
            phases_factory,
            init_all_derived_data(),
            "testrepo".to_string(),
        ))
    }
}

/// Used by tests
pub fn new_memblob_empty(blobstore: Option<Arc<dyn Blobstore>>) -> Result<BlobRepo> {
    TestRepoBuilder::new().maybe_blobstore(blobstore).build()
}

/// Used by cross-repo syncing tests
pub fn new_memblob_empty_with_id(
    blobstore: Option<Arc<dyn Blobstore>>,
    repo_id: RepositoryId,
) -> Result<BlobRepo> {
    TestRepoBuilder::new()
        .maybe_blobstore(blobstore)
        .id(repo_id)
        .build()
}

pub fn init_all_derived_data() -> DerivedDataConfig {
    DerivedDataConfig {
        scuba_table: None,
        derived_data_types: btreeset! {
            BlameRoot::NAME.to_string(),
            FilenodesOnlyPublic::NAME.to_string(),
            ChangesetInfo::NAME.to_string(),
            RootFastlog::NAME.to_string(),
            RootFsnodeId::NAME.to_string(),
            RootDeletedManifestId::NAME.to_string(),
            RootUnodeManifestId::NAME.to_string(),
            TreeHandle::NAME.to_string(),
        },
        unode_version: UnodeVersion::V2,
    }
}

// Creates all db tables except for filenodes on the same sqlite connection
pub fn new_memblob_with_sqlite_connection_with_id(
    con: SqliteConnection,
    repo_id: RepositoryId,
) -> Result<(BlobRepo, Connection)> {
    con.execute_batch(SqlBookmarks::CREATION_QUERY)?;
    con.execute_batch(SqlChangesets::CREATION_QUERY)?;
    con.execute_batch(SqlBonsaiGitMappingConnection::CREATION_QUERY)?;
    con.execute_batch(SqlBonsaiGlobalrevMapping::CREATION_QUERY)?;
    con.execute_batch(SqlBonsaiHgMapping::CREATION_QUERY)?;
    con.execute_batch(SqlPhasesFactory::CREATION_QUERY)?;
    con.execute_batch(SqlHgMutationStoreBuilder::CREATION_QUERY)?;
    let con = Connection::with_sqlite(con);

    new_memblob_with_connection_with_id(con.clone(), repo_id)
}

pub fn new_memblob_with_connection_with_id(
    con: Connection,
    repo_id: RepositoryId,
) -> Result<(BlobRepo, Connection)> {
    let repo_blobstore_args = RepoBlobstoreArgs::new(
        Arc::new(EagerMemblob::new()),
        None,
        repo_id,
        ScubaSampleBuilder::with_discard(),
    );

    let sql_connections = SqlConnections::new_single(con.clone());

    let phases_factory = SqlPhasesFactory::from_sql_connections(sql_connections.clone());

    Ok((
        blobrepo_new(
            Arc::new(SqlBookmarks::from_sql_connections(sql_connections.clone())),
            repo_blobstore_args,
            // Filenodes are intentionally created on another connection
            Arc::new(
                NewFilenodesBuilder::with_sqlite_in_memory()
                    .context(ErrorKind::StateOpen(StateOpenError::Filenodes))?
                    .build(),
            ),
            Arc::new(SqlChangesets::from_sql_connections(sql_connections.clone())),
            Arc::new(
                SqlBonsaiGitMappingConnection::from_sql_connections(sql_connections.clone())
                    .with_repo_id(repo_id.clone()),
            ),
            Arc::new(SqlBonsaiGlobalrevMapping::from_sql_connections(
                sql_connections.clone(),
            )),
            Arc::new(SqlBonsaiHgMapping::from_sql_connections(
                sql_connections.clone(),
            )),
            Arc::new(
                SqlHgMutationStoreBuilder::from_sql_connections(sql_connections)
                    .with_repo_id(repo_id),
            ),
            Arc::new(InProcessLease::new()),
            FilestoreConfig::default(),
            phases_factory,
            init_all_derived_data(),
            "testrepo".to_string(),
        ),
        con,
    ))
}

async fn new_development(
    fb: FacebookInit,
    sql_factory: &MetadataSqlFactory,
    blobstore: Arc<dyn Blobstore>,
    redacted_blobs: Option<HashMap<String, String>>,
    scuba_censored_table: Option<String>,
    repoid: RepositoryId,
    filestore_config: FilestoreConfig,
    bookmarks_cache_ttl: Option<Duration>,
    derived_data_config: DerivedDataConfig,
    reponame: String,
) -> Result<BlobRepo, Error> {
    let bookmarks = async {
        let bookmarks = sql_factory
            .open::<SqlBookmarks>()
            .compat()
            .await
            .context(ErrorKind::StateOpen(StateOpenError::Bookmarks))?;

        let bookmarks: Arc<dyn Bookmarks> = if let Some(ttl) = bookmarks_cache_ttl {
            Arc::new(CachedBookmarks::new(Arc::new(bookmarks), ttl))
        } else {
            Arc::new(bookmarks)
        };

        Ok(bookmarks)
    };

    let filenodes_builder = async {
        sql_factory
            .open_shardable::<NewFilenodesBuilder>()
            .compat()
            .await
            .context(ErrorKind::StateOpen(StateOpenError::Filenodes))
    };

    let changesets = async {
        sql_factory
            .open::<SqlChangesets>()
            .compat()
            .await
            .context(ErrorKind::StateOpen(StateOpenError::Changesets))
    };

    let bonsai_git_mapping = async {
        let conn = sql_factory
            .open::<SqlBonsaiGitMappingConnection>()
            .compat()
            .await
            .context(ErrorKind::StateOpen(StateOpenError::BonsaiGitMapping))?;

        Ok(conn.with_repo_id(repoid))
    };

    let bonsai_globalrev_mapping = async {
        sql_factory
            .open::<SqlBonsaiGlobalrevMapping>()
            .compat()
            .await
            .context(ErrorKind::StateOpen(StateOpenError::BonsaiGlobalrevMapping))
    };

    let bonsai_hg_mapping = async {
        sql_factory
            .open::<SqlBonsaiHgMapping>()
            .compat()
            .await
            .context(ErrorKind::StateOpen(StateOpenError::BonsaiHgMapping))
    };

    let hg_mutation_store = async {
        let conn = sql_factory
            .open::<SqlHgMutationStoreBuilder>()
            .compat()
            .await
            .context(ErrorKind::StateOpen(StateOpenError::HgMutationStore))?;

        Ok(conn.with_repo_id(repoid))
    };

    let phases_factory = async {
        sql_factory
            .open::<SqlPhasesFactory>()
            .compat()
            .await
            .context(ErrorKind::StateOpen(StateOpenError::Phases))
    };

    let (
        bookmarks,
        filenodes_builder,
        changesets,
        bonsai_git_mapping,
        bonsai_globalrev_mapping,
        bonsai_hg_mapping,
        hg_mutation_store,
        phases_factory,
    ) = try_join!(
        bookmarks,
        filenodes_builder,
        changesets,
        bonsai_git_mapping,
        bonsai_globalrev_mapping,
        bonsai_hg_mapping,
        hg_mutation_store,
        phases_factory,
    )?;

    let scuba_builder = ScubaSampleBuilder::with_opt_table(fb, scuba_censored_table);

    Ok(blobrepo_new(
        bookmarks,
        RepoBlobstoreArgs::new(blobstore, redacted_blobs, repoid, scuba_builder),
        Arc::new(filenodes_builder.build()),
        Arc::new(changesets),
        Arc::new(bonsai_git_mapping),
        Arc::new(bonsai_globalrev_mapping),
        Arc::new(bonsai_hg_mapping),
        Arc::new(hg_mutation_store),
        Arc::new(InProcessLease::new()),
        filestore_config,
        phases_factory,
        derived_data_config,
        reponame,
    ))
}

/// If the DB is remote then set up for a full production configuration.
/// In theory this could be with a local blobstore, but that would just be weird.
async fn new_production(
    fb: FacebookInit,
    sql_factory: &MetadataSqlFactory,
    blobstore: Arc<dyn Blobstore>,
    redacted_blobs: Option<HashMap<String, String>>,
    scuba_censored_table: Option<String>,
    repoid: RepositoryId,
    bookmarks_cache_ttl: Option<Duration>,
    filestore_config: FilestoreConfig,
    readonly_storage: ReadOnlyStorage,
    derived_data_config: DerivedDataConfig,
    reponame: String,
) -> Result<BlobRepo, Error> {
    let blob_pool = get_cache_pool(BLOBSTORE_BLOBS_CACHE_POOL)?;
    let presence_pool = get_cache_pool(BLOBSTORE_PRESENCE_CACHE_POOL)?;

    let blobstore = new_memcache_blobstore(fb, blobstore, "multiplexed", "")?;
    let blobstore = Arc::new(new_cachelib_blobstore_no_lease(
        blobstore,
        Arc::new(blob_pool),
        Arc::new(presence_pool),
    )) as Arc<dyn Blobstore>;

    let filenodes_pool = get_volatile_pool("filenodes")?;
    let filenodes_history_pool = get_volatile_pool("filenodes_history")?;
    let changesets_cache_pool = get_volatile_pool("changesets")?;
    let bonsai_hg_mapping_cache_pool = get_volatile_pool("bonsai_hg_mapping")?;
    let phases_cache_pool = get_volatile_pool("phases")?;
    let derived_data_lease = MemcacheOps::new(fb, "derived-data-lease", "")?;

    let filenodes_tier = sql_factory.tier_name_shardable::<NewFilenodesBuilder>()?;
    let filenodes_builder = sql_factory.open_shardable::<NewFilenodesBuilder>().compat();
    let bookmarks = sql_factory.open::<SqlBookmarks>().compat();
    let changesets = sql_factory.open::<SqlChangesets>().compat();
    let bonsai_git_mapping = async {
        let conn = sql_factory
            .open::<SqlBonsaiGitMappingConnection>()
            .compat()
            .await?;

        Ok(conn.with_repo_id(repoid))
    };
    let bonsai_globalrev_mapping = sql_factory.open::<SqlBonsaiGlobalrevMapping>().compat();
    let bonsai_hg_mapping = sql_factory.open::<SqlBonsaiHgMapping>().compat();
    let hg_mutation_store = async {
        let conn = sql_factory
            .open::<SqlHgMutationStoreBuilder>()
            .compat()
            .await?;

        Ok(conn.with_repo_id(repoid))
    };
    let phases_factory = sql_factory.open::<SqlPhasesFactory>().compat();

    // Wrap again to avoid any writes to memcache
    let blobstore = if readonly_storage.0 {
        Arc::new(ReadOnlyBlobstore::new(blobstore)) as Arc<dyn Blobstore>
    } else {
        blobstore
    };

    let (
        mut filenodes_builder,
        mut phases_factory,
        bonsai_git_mapping,
        bookmarks,
        changesets,
        bonsai_globalrev_mapping,
        bonsai_hg_mapping,
        hg_mutation_store,
    ) = try_join!(
        filenodes_builder,
        phases_factory,
        bonsai_git_mapping,
        bookmarks,
        changesets,
        bonsai_globalrev_mapping,
        bonsai_hg_mapping,
        hg_mutation_store,
    )?;

    filenodes_builder.enable_caching(
        fb,
        filenodes_pool,
        filenodes_history_pool,
        "newfilenodes",
        &filenodes_tier,
    );

    let bookmarks: Arc<dyn Bookmarks> = if let Some(ttl) = bookmarks_cache_ttl {
        Arc::new(CachedBookmarks::new(Arc::new(bookmarks), ttl))
    } else {
        Arc::new(bookmarks)
    };

    let changesets = Arc::new(CachingChangesets::new(
        fb,
        Arc::new(changesets),
        changesets_cache_pool,
    ));

    let bonsai_hg_mapping = CachingBonsaiHgMapping::new(
        fb,
        Arc::new(bonsai_hg_mapping),
        bonsai_hg_mapping_cache_pool,
    );

    phases_factory.enable_caching(fb, phases_cache_pool);
    let scuba_builder = ScubaSampleBuilder::with_opt_table(fb, scuba_censored_table);

    Ok(blobrepo_new(
        bookmarks,
        RepoBlobstoreArgs::new(blobstore, redacted_blobs, repoid, scuba_builder),
        Arc::new(filenodes_builder.build()) as Arc<dyn Filenodes>,
        changesets,
        Arc::new(bonsai_git_mapping),
        Arc::new(bonsai_globalrev_mapping),
        Arc::new(bonsai_hg_mapping),
        Arc::new(hg_mutation_store),
        Arc::new(derived_data_lease),
        filestore_config,
        phases_factory,
        derived_data_config,
        reponame,
    ))
}

fn get_volatile_pool(name: &str) -> Result<cachelib::VolatileLruCachePool> {
    cachelib::get_volatile_pool(name)?
        .ok_or_else(|| Error::from(ErrorKind::MissingCachePool(name.to_string())))
}

fn get_cache_pool(name: &str) -> Result<cachelib::LruCachePool> {
    cachelib::get_pool(name)
        .ok_or_else(|| Error::from(ErrorKind::MissingCachePool(name.to_string())))
}

pub fn blobrepo_new(
    bookmarks: Arc<dyn Bookmarks>,
    blobstore_args: RepoBlobstoreArgs,
    filenodes: Arc<dyn Filenodes>,
    changesets: Arc<dyn Changesets>,
    bonsai_git_mapping: Arc<dyn BonsaiGitMapping>,
    bonsai_globalrev_mapping: Arc<dyn BonsaiGlobalrevMapping>,
    bonsai_hg_mapping: Arc<dyn BonsaiHgMapping>,
    hg_mutation_store: Arc<dyn HgMutationStore>,
    derived_data_lease: Arc<dyn LeaseOps>,
    filestore_config: FilestoreConfig,
    phases_factory: SqlPhasesFactory,
    derived_data_config: DerivedDataConfig,
    reponame: String,
) -> BlobRepo {
    let attributes = {
        let mut attributes = TypeMap::new();
        attributes.insert::<dyn BonsaiHgMapping>(bonsai_hg_mapping);
        Arc::new(attributes)
    };
    BlobRepo::new_dangerous(
        bookmarks,
        blobstore_args,
        filenodes,
        changesets,
        bonsai_git_mapping,
        bonsai_globalrev_mapping,
        hg_mutation_store,
        derived_data_lease,
        filestore_config,
        phases_factory,
        derived_data_config,
        reponame,
        attributes,
    )
}
