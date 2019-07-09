// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{sync::Arc, time::Duration};

use cloned::cloned;
use failure_ext::prelude::*;
use failure_ext::{Error, Result};
use futures::{future::IntoFuture, Future};
use futures_ext::{BoxFuture, FutureExt};
use slog::{self, o, Discard, Drain, Logger};
use std::collections::HashMap;

use blobstore_factory::{make_blobstore, SqlFactory, SqliteFactory, XdbFactory};

use blobrepo::BlobRepo;
use blobrepo_errors::*;
use blobstore::Blobstore;
use bonsai_hg_mapping::{CachingBonsaiHgMapping, SqlBonsaiHgMapping};
use bookmarks::{Bookmarks, CachedBookmarks};
use cacheblob::{
    dummy::DummyLease, new_cachelib_blobstore_no_lease, new_memcache_blobstore, MemcacheOps,
};
use censoredblob::SqlCensoredContentStore;
use changeset_fetcher::{ChangesetFetcher, SimpleChangesetFetcher};
use changesets::{CachingChangesets, SqlChangesets};
use dbbookmarks::SqlBookmarks;
use filenodes::CachingFilenodes;
use memblob::EagerMemblob;
use metaconfig_types::{self, BlobConfig, Censoring, MetadataDBConfig, StorageConfig};
use mononoke_types::RepositoryId;
use sql_ext::myrouter_ready;
use sqlfilenodes::{SqlConstructors, SqlFilenodes};
use std::iter::FromIterator;

#[derive(Copy, Clone, PartialEq)]
pub enum Caching {
    Enabled,
    Disabled,
}

/// Construct a new BlobRepo with the given storage configuration. If the metadata DB is
/// remote (ie, MySQL), then it configures a full set of caches. Otherwise with local storage
/// it's assumed to be a test configuration.
///
/// The blobstore config is actually orthogonal to this, but it wouldn't make much sense to
/// configure a local blobstore with a remote db, or vice versa. There's no error checking
/// at this level (aside from disallowing a multiplexed blobstore with a local db).
pub fn open_blobrepo(
    logger: Logger,
    storage_config: StorageConfig,
    repoid: RepositoryId,
    myrouter_port: Option<u16>,
    caching: Caching,
    bookmarks_cache_ttl: Option<Duration>,
    censoring: Censoring,
) -> BoxFuture<BlobRepo, Error> {
    myrouter_ready(storage_config.dbconfig.get_db_address(), myrouter_port)
        .and_then(move |()| match storage_config.dbconfig {
            MetadataDBConfig::LocalDB { path } => do_open_blobrepo(
                logger,
                SqliteFactory::new(path),
                storage_config.blobstore,
                caching,
                repoid,
                myrouter_port,
                bookmarks_cache_ttl,
                censoring,
            )
            .left_future(),
            MetadataDBConfig::Mysql {
                db_address,
                sharded_filenodes,
            } => do_open_blobrepo(
                logger,
                XdbFactory::new(db_address, myrouter_port, sharded_filenodes),
                storage_config.blobstore,
                caching,
                repoid,
                myrouter_port,
                bookmarks_cache_ttl,
                censoring,
            )
            .right_future(),
        })
        .boxify()
}

fn do_open_blobrepo<T: SqlFactory>(
    logger: slog::Logger,
    sql_factory: T,
    blobconfig: BlobConfig,
    caching: Caching,
    repoid: RepositoryId,
    myrouter_port: Option<u16>,
    bookmarks_cache_ttl: Option<Duration>,
    censoring: Censoring,
) -> impl Future<Item = BlobRepo, Error = Error> {
    let uncensored_blobstore = make_blobstore(repoid, &blobconfig, &sql_factory, myrouter_port);

    let censored_blobs = match censoring {
        Censoring::Enabled => {
            let censored_blobs_store: Result<Arc<SqlCensoredContentStore>> = sql_factory.open();

            censored_blobs_store
                .into_future()
                .and_then(move |censored_store| {
                    let censored_blobs = censored_store
                        .get_all_censored_blobs()
                        .map_err(Error::from)
                        .map(HashMap::from_iter);
                    Some(censored_blobs)
                })
                .left_future()
        }
        Censoring::Disabled => Ok(None).into_future().right_future(),
    };

    uncensored_blobstore.join(censored_blobs).and_then(
        move |(uncensored_blobstore, censored_blobs)| match caching {
            Caching::Disabled => new_development(
                logger,
                &sql_factory,
                uncensored_blobstore,
                censored_blobs,
                repoid,
            ),
            Caching::Enabled => new_production(
                logger,
                &sql_factory,
                uncensored_blobstore,
                censored_blobs,
                repoid,
                bookmarks_cache_ttl,
            ),
        },
    )
}

/// Used by tests
pub fn new_memblob_empty(
    logger: Option<Logger>,
    blobstore: Option<Arc<dyn Blobstore>>,
) -> Result<BlobRepo> {
    Ok(BlobRepo::new(
        logger.unwrap_or(Logger::root(Discard {}.ignore_res(), o!())),
        Arc::new(SqlBookmarks::with_sqlite_in_memory()?),
        blobstore.unwrap_or_else(|| Arc::new(EagerMemblob::new())),
        None,
        Arc::new(
            SqlFilenodes::with_sqlite_in_memory()
                .chain_err(ErrorKind::StateOpen(StateOpenError::Filenodes))?,
        ),
        Arc::new(
            SqlChangesets::with_sqlite_in_memory()
                .chain_err(ErrorKind::StateOpen(StateOpenError::Changesets))?,
        ),
        Arc::new(
            SqlBonsaiHgMapping::with_sqlite_in_memory()
                .chain_err(ErrorKind::StateOpen(StateOpenError::BonsaiHgMapping))?,
        ),
        RepositoryId::new(0),
        Arc::new(DummyLease {}),
    ))
}

/// Create a new BlobRepo with purely local state. (Well, it could be a remote blobstore, but
/// that would be weird to use with a local metadata db.)
fn new_development<T: SqlFactory>(
    logger: Logger,
    sql_factory: &T,
    blobstore: Arc<Blobstore>,
    censored_blobs: Option<HashMap<String, String>>,
    repoid: RepositoryId,
) -> Result<BlobRepo> {
    let bookmarks: Arc<SqlBookmarks> = sql_factory
        .open()
        .chain_err(ErrorKind::StateOpen(StateOpenError::Bookmarks))?;
    let filenodes: Arc<SqlFilenodes> = sql_factory
        .open()
        .chain_err(ErrorKind::StateOpen(StateOpenError::Filenodes))?;
    let changesets: Arc<SqlChangesets> = sql_factory
        .open()
        .chain_err(ErrorKind::StateOpen(StateOpenError::Changesets))?;
    let bonsai_hg_mapping: Arc<SqlBonsaiHgMapping> = sql_factory
        .open()
        .chain_err(ErrorKind::StateOpen(StateOpenError::BonsaiHgMapping))?;

    Ok(BlobRepo::new(
        logger,
        bookmarks,
        blobstore,
        censored_blobs,
        filenodes,
        changesets,
        bonsai_hg_mapping,
        repoid,
        Arc::new(DummyLease {}),
    ))
}

/// If the DB is remote then set up for a full production configuration.
/// In theory this could be with a local blobstore, but that would just be weird.
fn new_production<T: SqlFactory>(
    logger: Logger,
    sql_factory: &T,
    blobstore: Arc<Blobstore>,
    censored_blobs: Option<HashMap<String, String>>,
    repoid: RepositoryId,
    bookmarks_cache_ttl: Option<Duration>,
) -> Result<BlobRepo> {
    let blobstore = new_memcache_blobstore(blobstore, "multiplexed", "")?;
    let blob_pool = Arc::new(cachelib::get_pool("blobstore-blobs").ok_or(Error::from(
        ErrorKind::MissingCachePool("blobstore-blobs".to_string()),
    ))?);
    let presence_pool = Arc::new(cachelib::get_pool("blobstore-presence").ok_or(Error::from(
        ErrorKind::MissingCachePool("blobstore-presence".to_string()),
    ))?);
    let blobstore = Arc::new(new_cachelib_blobstore_no_lease(
        blobstore,
        blob_pool,
        presence_pool,
    ));

    let filenodes_pool = cachelib::get_volatile_pool("filenodes")?.ok_or(Error::from(
        ErrorKind::MissingCachePool("filenodes".to_string()),
    ))?;
    let (filenodes_tier, filenodes): (String, Arc<SqlFilenodes>) = sql_factory.open_filenodes()?;

    let filenodes =
        CachingFilenodes::new(filenodes, filenodes_pool, "sqlfilenodes", &filenodes_tier);

    let bookmarks: Arc<dyn Bookmarks> = {
        let bookmarks: Arc<SqlBookmarks> = sql_factory.open()?;
        if let Some(ttl) = bookmarks_cache_ttl {
            Arc::new(CachedBookmarks::new(bookmarks, ttl))
        } else {
            bookmarks
        }
    };

    let changesets: Arc<SqlChangesets> = sql_factory.open()?;
    let changesets_cache_pool = cachelib::get_volatile_pool("changesets")?.ok_or(Error::from(
        ErrorKind::MissingCachePool("changesets".to_string()),
    ))?;
    let changesets = CachingChangesets::new(changesets, changesets_cache_pool.clone());
    let changesets = Arc::new(changesets);

    let bonsai_hg_mapping: Arc<SqlBonsaiHgMapping> = sql_factory.open()?;
    let bonsai_hg_mapping = CachingBonsaiHgMapping::new(
        bonsai_hg_mapping,
        cachelib::get_volatile_pool("bonsai_hg_mapping")?.ok_or(Error::from(
            ErrorKind::MissingCachePool("bonsai_hg_mapping".to_string()),
        ))?,
    );

    let changeset_fetcher_factory = {
        cloned!(changesets, repoid);
        move || {
            let res: Arc<ChangesetFetcher + Send + Sync> = Arc::new(SimpleChangesetFetcher::new(
                changesets.clone(),
                repoid.clone(),
            ));
            res
        }
    };

    Ok(BlobRepo::new_with_changeset_fetcher_factory(
        logger,
        bookmarks,
        blobstore,
        censored_blobs,
        Arc::new(filenodes),
        changesets,
        Arc::new(bonsai_hg_mapping),
        repoid,
        Arc::new(changeset_fetcher_factory),
        Arc::new(MemcacheOps::new("bonsai-hg-generation", "")?),
    ))
}
