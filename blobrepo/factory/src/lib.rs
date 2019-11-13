/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use blobrepo::BlobRepo;
use blobrepo_errors::*;
use blobstore::Blobstore;
use blobstore_factory::{make_blobstore, make_sql_factory, SqlFactory};
use bonsai_globalrev_mapping::SqlBonsaiGlobalrevMapping;
use bonsai_hg_mapping::{CachingBonsaiHgMapping, SqlBonsaiHgMapping};
use bookmarks::{Bookmarks, CachedBookmarks};
use cacheblob::{
    new_cachelib_blobstore_no_lease, new_memcache_blobstore, InProcessLease, MemcacheOps,
};
use changeset_fetcher::{ChangesetFetcher, SimpleChangesetFetcher};
use changesets::{CachingChangesets, SqlChangesets};
use cloned::cloned;
use dbbookmarks::SqlBookmarks;
use failure_ext::prelude::*;
use failure_ext::{Error, Result};
use fbinit::FacebookInit;
use filenodes::CachingFilenodes;
use filestore::FilestoreConfig;
use futures::{future::IntoFuture, Future};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use memblob::EagerMemblob;
use metaconfig_types::{self, FilestoreParams, Redaction, StorageConfig};
use mononoke_types::RepositoryId;
use readonlyblob::ReadOnlyBlobstore;
use redactedblobstore::SqlRedactedContentStore;
use repo_blobstore::RepoBlobstoreArgs;
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};
use slog::Logger;
use sqlfilenodes::{SqlConstructors, SqlFilenodes};
use std::{collections::HashMap, iter::FromIterator, sync::Arc, time::Duration};

pub use blobstore_factory::ReadOnlyStorage;

#[derive(Copy, Clone, PartialEq)]
pub enum Caching {
    Enabled,
    Disabled,
    CachelibOnlyBlobstore,
}

const BLOBSTORE_BLOBS_CACHE_POOL: &'static str = "blobstore-blobs";
const BLOBSTORE_PRESENCE_CACHE_POOL: &'static str = "blobstore-presence";

/// Construct a new BlobRepo with the given storage configuration. If the metadata DB is
/// remote (ie, MySQL), then it configures a full set of caches. Otherwise with local storage
/// it's assumed to be a test configuration.
///
/// The blobstore config is actually orthogonal to this, but it wouldn't make much sense to
/// configure a local blobstore with a remote db, or vice versa. There's no error checking
/// at this level (aside from disallowing a multiplexed blobstore with a local db).
pub fn open_blobrepo(
    fb: FacebookInit,
    storage_config: StorageConfig,
    repoid: RepositoryId,
    myrouter_port: Option<u16>,
    caching: Caching,
    bookmarks_cache_ttl: Option<Duration>,
    redaction: Redaction,
    scuba_censored_table: Option<String>,
    filestore_params: Option<FilestoreParams>,
    readonly_storage: ReadOnlyStorage,
    logger: Logger,
) -> BoxFuture<BlobRepo, Error> {
    let sql_fut = make_sql_factory(
        storage_config.dbconfig,
        myrouter_port,
        readonly_storage,
        logger.clone(),
    );
    let blobconfig = storage_config.blobstore;
    let datasources = sql_fut
        .map(move |sql_factory| {
            (
                make_blobstore(
                    fb,
                    &blobconfig,
                    &sql_factory,
                    myrouter_port,
                    readonly_storage,
                ),
                sql_factory,
            )
        })
        .boxify();

    open_blobrepo_given_datasources(
        fb,
        datasources,
        repoid,
        caching,
        bookmarks_cache_ttl,
        redaction,
        scuba_censored_table,
        filestore_params,
        readonly_storage,
    )
    .boxify()
}

/// Expose for graph walker that has storage open already
pub fn open_blobrepo_given_datasources(
    fb: FacebookInit,
    datasources: BoxFuture<(BoxFuture<Arc<dyn Blobstore>, Error>, SqlFactory), Error>,
    repoid: RepositoryId,
    caching: Caching,
    bookmarks_cache_ttl: Option<Duration>,
    redaction: Redaction,
    scuba_censored_table: Option<String>,
    filestore_params: Option<FilestoreParams>,
    readonly_storage: ReadOnlyStorage,
) -> impl Future<Item = BlobRepo, Error = Error> {
    datasources.and_then(move |(unredacted_blobstore, sql_factory)| {
        let redacted_blobs = match redaction {
            Redaction::Enabled => sql_factory
                .open::<SqlRedactedContentStore>()
                .and_then(move |redacted_store| {
                    let redacted_blobs = redacted_store
                        .get_all_redacted_blobs()
                        .map_err(Error::from)
                        .map(HashMap::from_iter);
                    Some(redacted_blobs)
                })
                .left_future(),
            Redaction::Disabled => Ok(None).into_future().right_future(),
        }
        .boxify();

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
            .unwrap_or(FilestoreConfig::default());

        match caching {
            Caching::Disabled | Caching::CachelibOnlyBlobstore => {
                let blobstore = if caching == Caching::CachelibOnlyBlobstore {
                    // Use cachelib
                    let blob_pool = try_boxfuture!(get_cache_pool(BLOBSTORE_BLOBS_CACHE_POOL));
                    let presence_pool =
                        try_boxfuture!(get_cache_pool(BLOBSTORE_PRESENCE_CACHE_POOL));

                    unredacted_blobstore
                        .map(move |s| {
                            let s: Arc<dyn Blobstore> = Arc::new(new_cachelib_blobstore_no_lease(
                                s,
                                Arc::new(blob_pool),
                                Arc::new(presence_pool),
                            ));
                            s
                        })
                        .boxify()
                } else {
                    unredacted_blobstore
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
                )
            }
            Caching::Enabled => new_production(
                fb,
                &sql_factory,
                unredacted_blobstore,
                redacted_blobs,
                scuba_censored_table,
                repoid,
                bookmarks_cache_ttl,
                filestore_config,
                readonly_storage,
            ),
        }
    })
}

/// Used by tests
pub fn new_memblob_empty(blobstore: Option<Arc<dyn Blobstore>>) -> Result<BlobRepo> {
    new_memblob_empty_with_id(blobstore, RepositoryId::new(0))
}

/// Used by cross-repo syncing tests
pub fn new_memblob_empty_with_id(
    blobstore: Option<Arc<dyn Blobstore>>,
    repo_id: RepositoryId,
) -> Result<BlobRepo> {
    let repo_blobstore_args = RepoBlobstoreArgs::new(
        blobstore.unwrap_or_else(|| Arc::new(EagerMemblob::new())),
        None,
        repo_id,
        ScubaSampleBuilder::with_discard(),
    );

    Ok(BlobRepo::new(
        Arc::new(SqlBookmarks::with_sqlite_in_memory()?),
        repo_blobstore_args,
        Arc::new(
            SqlFilenodes::with_sqlite_in_memory()
                .chain_err(ErrorKind::StateOpen(StateOpenError::Filenodes))?,
        ),
        Arc::new(
            SqlChangesets::with_sqlite_in_memory()
                .chain_err(ErrorKind::StateOpen(StateOpenError::Changesets))?,
        ),
        Arc::new(
            SqlBonsaiGlobalrevMapping::with_sqlite_in_memory()
                .chain_err(ErrorKind::StateOpen(StateOpenError::BonsaiGlobalrevMapping))?,
        ),
        Arc::new(
            SqlBonsaiHgMapping::with_sqlite_in_memory()
                .chain_err(ErrorKind::StateOpen(StateOpenError::BonsaiHgMapping))?,
        ),
        Arc::new(InProcessLease::new()),
        FilestoreConfig::default(),
    ))
}

fn new_development(
    fb: FacebookInit,
    sql_factory: &SqlFactory,
    unredacted_blobstore: BoxFuture<Arc<dyn Blobstore>, Error>,
    redacted_blobs: BoxFuture<Option<HashMap<String, String>>, Error>,
    scuba_censored_table: Option<String>,
    repoid: RepositoryId,
    filestore_config: FilestoreConfig,
    bookmarks_cache_ttl: Option<Duration>,
) -> BoxFuture<BlobRepo, Error> {
    let bookmarks = sql_factory
        .open::<SqlBookmarks>()
        .chain_err(ErrorKind::StateOpen(StateOpenError::Bookmarks))
        .from_err()
        .map(move |bookmarks| {
            let bookmarks: Arc<dyn Bookmarks> = if let Some(ttl) = bookmarks_cache_ttl {
                Arc::new(CachedBookmarks::new(bookmarks, ttl))
            } else {
                bookmarks
            };

            bookmarks
        });

    let filenodes = sql_factory
        .open_filenodes()
        .chain_err(ErrorKind::StateOpen(StateOpenError::Filenodes))
        .from_err()
        .map(|(_tier, filenodes)| filenodes);

    let changesets = sql_factory
        .open::<SqlChangesets>()
        .chain_err(ErrorKind::StateOpen(StateOpenError::Changesets))
        .from_err();

    let bonsai_globalrev_mapping = sql_factory
        .open::<SqlBonsaiGlobalrevMapping>()
        .chain_err(ErrorKind::StateOpen(StateOpenError::BonsaiGlobalrevMapping))
        .from_err();

    let bonsai_hg_mapping = sql_factory
        .open::<SqlBonsaiHgMapping>()
        .chain_err(ErrorKind::StateOpen(StateOpenError::BonsaiHgMapping))
        .from_err();

    bookmarks
        .join3(unredacted_blobstore, redacted_blobs)
        .join5(
            filenodes,
            changesets,
            bonsai_globalrev_mapping,
            bonsai_hg_mapping,
        )
        .map({
            move |(
                (bookmarks, blobstore, redacted_blobs),
                filenodes,
                changesets,
                bonsai_globalrev_mapping,
                bonsai_hg_mapping,
            )| {
                let scuba_builder = ScubaSampleBuilder::with_opt_table(fb, scuba_censored_table);

                BlobRepo::new(
                    bookmarks,
                    RepoBlobstoreArgs::new(blobstore, redacted_blobs, repoid, scuba_builder),
                    filenodes,
                    changesets,
                    bonsai_globalrev_mapping,
                    bonsai_hg_mapping,
                    Arc::new(InProcessLease::new()),
                    filestore_config,
                )
            }
        })
        .boxify()
}

/// If the DB is remote then set up for a full production configuration.
/// In theory this could be with a local blobstore, but that would just be weird.
fn new_production(
    fb: FacebookInit,
    sql_factory: &SqlFactory,
    blobstore: BoxFuture<Arc<dyn Blobstore>, Error>,
    redacted_blobs: BoxFuture<Option<HashMap<String, String>>, Error>,
    scuba_censored_table: Option<String>,
    repoid: RepositoryId,
    bookmarks_cache_ttl: Option<Duration>,
    filestore_config: FilestoreConfig,
    readonly_storage: ReadOnlyStorage,
) -> BoxFuture<BlobRepo, Error> {
    fn get_volatile_pool(name: &str) -> Result<cachelib::VolatileLruCachePool> {
        let err = Error::from(ErrorKind::MissingCachePool(name.to_string()));
        cachelib::get_volatile_pool(name)?.ok_or(err)
    }

    let blob_pool = try_boxfuture!(get_cache_pool(BLOBSTORE_BLOBS_CACHE_POOL));
    let presence_pool = try_boxfuture!(get_cache_pool(BLOBSTORE_PRESENCE_CACHE_POOL));

    let blobstore = blobstore
        .and_then(move |blobstore| new_memcache_blobstore(fb, blobstore, "multiplexed", ""));
    let blobstore = blobstore.map(|blobstore| {
        Arc::new(new_cachelib_blobstore_no_lease(
            blobstore,
            Arc::new(blob_pool),
            Arc::new(presence_pool),
        )) as Arc<dyn Blobstore>
    });

    let filenodes_pool = try_boxfuture!(get_volatile_pool("filenodes"));
    let changesets_cache_pool = try_boxfuture!(get_volatile_pool("changesets"));
    let bonsai_hg_mapping_cache_pool = try_boxfuture!(get_volatile_pool("bonsai_hg_mapping"));

    let derive_data_lease = try_boxfuture!(MemcacheOps::new(fb, "derived-data-lease", ""));

    let filenodes_tier_and_filenodes = sql_factory.open_filenodes();
    let bookmarks = sql_factory.open::<SqlBookmarks>();
    let changesets = sql_factory.open::<SqlChangesets>();
    let bonsai_globalrev_mapping = sql_factory.open::<SqlBonsaiGlobalrevMapping>();
    let bonsai_hg_mapping = sql_factory.open::<SqlBonsaiHgMapping>();

    // Wrap again to avoid any writes to memcache
    let blobstore = if readonly_storage.0 {
        blobstore
            .map(|inner| Arc::new(ReadOnlyBlobstore::new(inner)) as Arc<dyn Blobstore>)
            .left_future()
    } else {
        blobstore.right_future()
    };

    filenodes_tier_and_filenodes
        .join3(blobstore, redacted_blobs)
        .join5(
            bookmarks,
            changesets,
            bonsai_globalrev_mapping,
            bonsai_hg_mapping,
        )
        .map(
            move |(
                ((filenodes_tier, filenodes), blobstore, redacted_blobs),
                bookmarks,
                changesets,
                bonsai_globalrev_mapping,
                bonsai_hg_mapping,
            )| {
                let filenodes = CachingFilenodes::new(
                    fb,
                    filenodes,
                    filenodes_pool,
                    "sqlfilenodes",
                    &filenodes_tier,
                );

                let bookmarks: Arc<dyn Bookmarks> = {
                    if let Some(ttl) = bookmarks_cache_ttl {
                        Arc::new(CachedBookmarks::new(bookmarks, ttl))
                    } else {
                        bookmarks
                    }
                };

                let changesets = Arc::new(CachingChangesets::new(
                    fb,
                    changesets,
                    changesets_cache_pool,
                ));

                let bonsai_hg_mapping = CachingBonsaiHgMapping::new(
                    fb,
                    bonsai_hg_mapping,
                    bonsai_hg_mapping_cache_pool,
                );

                let changeset_fetcher_factory = {
                    cloned!(changesets, repoid);
                    move || {
                        let res: Arc<dyn ChangesetFetcher + Send + Sync> = Arc::new(
                            SimpleChangesetFetcher::new(changesets.clone(), repoid.clone()),
                        );
                        res
                    }
                };

                let scuba_builder = ScubaSampleBuilder::with_opt_table(fb, scuba_censored_table);

                BlobRepo::new_with_changeset_fetcher_factory(
                    bookmarks,
                    RepoBlobstoreArgs::new(blobstore, redacted_blobs, repoid, scuba_builder),
                    Arc::new(filenodes),
                    changesets,
                    bonsai_globalrev_mapping,
                    Arc::new(bonsai_hg_mapping),
                    Arc::new(changeset_fetcher_factory),
                    Arc::new(derive_data_lease),
                    filestore_config,
                )
            },
        )
        .boxify()
}

fn get_cache_pool(name: &str) -> Result<cachelib::LruCachePool> {
    let err = Error::from(ErrorKind::MissingCachePool(name.to_string()));
    cachelib::get_pool(name).ok_or(err)
}
