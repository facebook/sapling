// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{path::PathBuf, sync::Arc, time::Duration};

use cloned::cloned;
use failure_ext::prelude::*;
use failure_ext::{Error, Result};
use futures::{
    future::{self, IntoFuture},
    Future,
};
use futures_ext::{BoxFuture, FutureExt};
use slog::{self, o, Discard, Drain, Logger};
use std::collections::HashMap;

use blobrepo::BlobRepo;
use blobrepo_errors::*;
use blobstore::{Blobstore, DisabledBlob};
use blobstore_sync_queue::SqlBlobstoreSyncQueue;
use bonsai_hg_mapping::{CachingBonsaiHgMapping, SqlBonsaiHgMapping};
use bookmarks::{Bookmarks, CachedBookmarks};
use cacheblob::{
    dummy::DummyLease, new_cachelib_blobstore_no_lease, new_memcache_blobstore, MemcacheOps,
};
use censoredblob::SqlCensoredContentStore;
use changeset_fetcher::{ChangesetFetcher, SimpleChangesetFetcher};
use changesets::{CachingChangesets, SqlChangesets};
use dbbookmarks::SqlBookmarks;
use fileblob::Fileblob;
use filenodes::CachingFilenodes;
use glusterblob::Glusterblob;
use manifoldblob::ThriftManifoldBlob;
use memblob::EagerMemblob;
use metaconfig_types::{
    self, BlobConfig, Censoring, MetadataDBConfig, ShardedFilenodesParams, StorageConfig,
};
use mononoke_types::RepositoryId;
use multiplexedblob::{MultiplexedBlobstore, ScrubBlobstore};
use prefixblob::PrefixBlobstore;
use rocksblob::Rocksblob;
use rocksdb;
use scuba::ScubaClient;
use sql_ext::myrouter_ready;
use sqlblob::Sqlblob;
use sqlfilenodes::{SqlConstructors, SqlFilenodes};
use std::iter::FromIterator;

#[derive(Copy, Clone, PartialEq)]
pub enum Caching {
    Enabled,
    Disabled,
}

#[derive(Copy, Clone, PartialEq)]
pub enum Scrubbing {
    Enabled,
    Disabled,
}

trait SqlFactory: Send + Sync {
    /// Open an arbitrary struct implementing SqlConstructors
    fn open<T: SqlConstructors>(&self) -> Result<Arc<T>>;

    /// Open SqlFilenodes, and return a tier name and the struct.
    fn open_filenodes(&self) -> Result<(String, Arc<SqlFilenodes>)>;
}

struct XdbFactory {
    db_address: String,
    myrouter_port: Option<u16>,
    sharded_filenodes: Option<ShardedFilenodesParams>,
}

impl XdbFactory {
    fn new(
        db_address: String,
        myrouter_port: Option<u16>,
        sharded_filenodes: Option<ShardedFilenodesParams>,
    ) -> Self {
        XdbFactory {
            db_address,
            myrouter_port,
            sharded_filenodes,
        }
    }
}

impl SqlFactory for XdbFactory {
    fn open<T: SqlConstructors>(&self) -> Result<Arc<T>> {
        Ok(Arc::new(T::with_xdb(
            self.db_address.clone(),
            self.myrouter_port,
        )?))
    }

    fn open_filenodes(&self) -> Result<(String, Arc<SqlFilenodes>)> {
        let (tier, filenodes) = match (self.sharded_filenodes.clone(), self.myrouter_port) {
            (
                Some(ShardedFilenodesParams {
                    shard_map,
                    shard_num,
                }),
                Some(port),
            ) => {
                let conn = SqlFilenodes::with_sharded_myrouter(&shard_map, port, shard_num.into())?;
                (shard_map, Arc::new(conn))
            }
            (
                Some(ShardedFilenodesParams {
                    shard_map,
                    shard_num,
                }),
                None,
            ) => {
                let conn = SqlFilenodes::with_sharded_raw_xdb(&shard_map, shard_num.into())?;
                (shard_map, Arc::new(conn))
            }
            (None, port) => {
                let conn = SqlFilenodes::with_xdb(self.db_address.clone(), port)?;
                (self.db_address.clone(), Arc::new(conn))
            }
        };

        Ok((tier, filenodes))
    }
}

struct SqliteFactory {
    path: PathBuf,
}

impl SqliteFactory {
    fn new(path: PathBuf) -> Self {
        SqliteFactory { path }
    }
}

impl SqlFactory for SqliteFactory {
    fn open<T: SqlConstructors>(&self) -> Result<Arc<T>> {
        Ok(Arc::new(T::with_sqlite_path(self.path.join(T::LABEL))?))
    }

    fn open_filenodes(&self) -> Result<(String, Arc<SqlFilenodes>)> {
        let filenodes: Arc<SqlFilenodes> = self.open()?;
        Ok(("sqlite".to_string(), filenodes))
    }
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

/// Construct a blobstore according to the specification. The multiplexed blobstore
/// needs an SQL DB for its queue, as does the MySQL blobstore.
fn make_blobstore<T: SqlFactory>(
    repoid: RepositoryId,
    blobconfig: &BlobConfig,
    sql_factory: &T,
    myrouter_port: Option<u16>,
) -> BoxFuture<Arc<dyn Blobstore>, Error> {
    use BlobConfig::*;

    match blobconfig {
        Disabled => {
            Ok(Arc::new(DisabledBlob::new("Disabled by configuration")) as Arc<dyn Blobstore>)
                .into_future()
                .boxify()
        }

        Files { path } => Fileblob::create(path.join("blobs"))
            .chain_err(ErrorKind::StateOpen(StateOpenError::Blobstore))
            .map(|store| Arc::new(store) as Arc<dyn Blobstore>)
            .map_err(Error::from)
            .into_future()
            .boxify(),

        Rocks { path } => {
            let options = rocksdb::Options::new().create_if_missing(true);
            Rocksblob::open_with_options(path.join("blobs"), options)
                .chain_err(ErrorKind::StateOpen(StateOpenError::Blobstore))
                .map(|store| Arc::new(store) as Arc<dyn Blobstore>)
                .map_err(Error::from)
                .into_future()
                .boxify()
        }

        Sqlite { path } => Sqlblob::with_sqlite_path(repoid, path.join("blobs"))
            .chain_err(ErrorKind::StateOpen(StateOpenError::Blobstore))
            .map_err(Error::from)
            .map(|store| Arc::new(store) as Arc<dyn Blobstore>)
            .into_future()
            .boxify(),

        Manifold { bucket, prefix } => ThriftManifoldBlob::new(bucket.clone())
            .map({
                cloned!(prefix);
                move |manifold| PrefixBlobstore::new(manifold, format!("flat/{}", prefix))
            })
            .chain_err(ErrorKind::StateOpen(StateOpenError::Blobstore))
            .map_err(Error::from)
            .map(|store| Arc::new(store) as Arc<dyn Blobstore>)
            .into_future()
            .boxify(),

        Gluster {
            tier,
            export,
            basepath,
        } => Glusterblob::with_smc(tier.clone(), export.clone(), basepath.clone())
            .map(|store| Arc::new(store) as Arc<dyn Blobstore>)
            .boxify(),

        Mysql {
            shard_map,
            shard_num,
        } => if let Some(myrouter_port) = myrouter_port {
            Sqlblob::with_myrouter(repoid, shard_map, myrouter_port, *shard_num)
        } else {
            Sqlblob::with_raw_xdb_shardmap(repoid, shard_map, *shard_num)
        }
        .map(|store| Arc::new(store) as Arc<dyn Blobstore>)
        .into_future()
        .boxify(),

        Multiplexed {
            scuba_table,
            blobstores,
        } => {
            let queue: Result<Arc<SqlBlobstoreSyncQueue>> = sql_factory.open();
            let components: Vec<_> = blobstores
                .iter()
                .map({
                    move |(blobstoreid, config)| {
                        cloned!(blobstoreid);
                        make_blobstore(repoid, config, sql_factory, myrouter_port)
                            .map({ move |store| (blobstoreid, store) })
                    }
                })
                .collect();

            queue
                .into_future()
                .and_then({
                    cloned!(scuba_table);
                    move |queue| {
                        future::join_all(components).map({
                            move |components| {
                                Arc::new(MultiplexedBlobstore::new(
                                    repoid,
                                    components,
                                    queue,
                                    scuba_table.map(|table| Arc::new(ScubaClient::new(table))),
                                )) as Arc<dyn Blobstore>
                            }
                        })
                    }
                })
                .boxify()
        }
        Scrub {
            scuba_table,
            blobstores,
        } => {
            let queue: Result<Arc<SqlBlobstoreSyncQueue>> = sql_factory.open();
            let components: Vec<_> = blobstores
                .iter()
                .map({
                    move |(blobstoreid, config)| {
                        cloned!(blobstoreid);
                        make_blobstore(repoid, config, sql_factory, myrouter_port)
                            .map({ move |store| (blobstoreid, store) })
                    }
                })
                .collect();

            queue
                .into_future()
                .and_then({
                    cloned!(scuba_table);
                    move |queue| {
                        future::join_all(components).map({
                            move |components| {
                                Arc::new(ScrubBlobstore::new(
                                    repoid,
                                    components,
                                    queue,
                                    scuba_table.map(|table| Arc::new(ScubaClient::new(table))),
                                )) as Arc<dyn Blobstore>
                            }
                        })
                    }
                })
                .boxify()
        }
    }
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
