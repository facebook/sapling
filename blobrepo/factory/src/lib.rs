// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{path::Path, sync::Arc, time::Duration};

use cloned::cloned;
use failure_ext::prelude::*;
use failure_ext::{err_msg, Error, Result};
use futures::{
    future::{self, result, IntoFuture},
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
use censoredblob::{CensoredBlob, SqlCensoredContentStore};
use changeset_fetcher::{ChangesetFetcher, SimpleChangesetFetcher};
use changesets::{CachingChangesets, SqlChangesets};
use dbbookmarks::SqlBookmarks;
use fileblob::Fileblob;
use filenodes::CachingFilenodes;
use glusterblob::Glusterblob;
use manifoldblob::ThriftManifoldBlob;
use memblob::EagerMemblob;
use metaconfig_types::{self, BlobConfig, MetadataDBConfig, ShardedFilenodesParams, StorageConfig};
use mononoke_types::RepositoryId;
use multiplexedblob::MultiplexedBlobstore;
use prefixblob::PrefixBlobstore;
use rocksblob::Rocksblob;
use rocksdb;
use scuba::ScubaClient;
use sql_ext::myrouter_ready;
use sqlblob::Sqlblob;
use sqlfilenodes::{SqlConstructors, SqlFilenodes};
use std::iter::FromIterator;

/// Construct a new BlobRepo with the given storage configuration. If the metadata DB is
/// remote (ie, MySQL), then it configures a full set of caches. Otherwise with local storage
/// it's assumed to be a test configuration.
///
/// The blobstore config is actually orthogonal to this, but it wouldn't make much sense to
/// configure a local blobstore with a remote db, or vice versa. There's no error checking
/// at this level (aside from disallowing a multiplexed blobstore with a local db).
pub fn open_blobrepo(
    logger: slog::Logger,
    storage_config: StorageConfig,
    repoid: RepositoryId,
    myrouter_port: Option<u16>,
    bookmarks_cache_ttl: Option<Duration>,
) -> BoxFuture<BlobRepo, Error> {
    myrouter_ready(storage_config.dbconfig.get_db_address(), myrouter_port)
        .and_then(move |()| {
            let uncensored_blobstore = make_blobstore(
                repoid,
                &storage_config.blobstore,
                &storage_config.dbconfig,
                myrouter_port,
            );

            let censored_blobs_store = result(match storage_config.dbconfig.clone() {
                MetadataDBConfig::LocalDB { ref path } => {
                    SqlCensoredContentStore::with_sqlite_path(path.join("censored_contents"))
                        .map(|item| Arc::new(item))
                }
                MetadataDBConfig::Mysql {
                    db_address,
                    sharded_filenodes: _,
                } => open_xdb::<SqlCensoredContentStore>(&db_address, myrouter_port),
            });

            let censored_blobs = censored_blobs_store.and_then(move |censored_store| {
                censored_store
                    .get_all_censored_blobs()
                    .map_err(Error::from)
                    .map(HashMap::from_iter)
            });

            uncensored_blobstore.join(censored_blobs).and_then(
                move |(uncensored_blobstore, censored_blobs)| {
                    let blobstore =
                        Arc::new(CensoredBlob::new(uncensored_blobstore, censored_blobs));
                    match storage_config.dbconfig {
                        MetadataDBConfig::LocalDB { path } => {
                            new_local(logger, &path, blobstore, repoid)
                        }
                        MetadataDBConfig::Mysql {
                            db_address,
                            sharded_filenodes,
                        } => new_remote(
                            logger,
                            db_address,
                            sharded_filenodes,
                            blobstore,
                            repoid,
                            myrouter_port,
                            bookmarks_cache_ttl,
                        ),
                    }
                },
            )
        })
        .boxify()
}

/// Construct a blobstore according to the specification. The multiplexed blobstore
/// needs an SQL DB for its queue, as does the MySQL blobstore.
fn make_blobstore(
    repoid: RepositoryId,
    blobconfig: &BlobConfig,
    dbconfig: &MetadataDBConfig,
    myrouter_port: Option<u16>,
) -> BoxFuture<Arc<Blobstore>, Error> {
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
            let queue = if dbconfig.is_local() {
                dbconfig
                    .get_local_address()
                    .ok_or_else(|| err_msg("Local db path is not specified"))
                    .and_then(|path| {
                        Ok(Arc::new(SqlBlobstoreSyncQueue::with_sqlite_path(
                            path.join("blobstore_sync_queue"),
                        )?))
                    })
                    .into_future()
            } else {
                dbconfig
                    .get_db_address()
                    .ok_or_else(|| err_msg("remote db address is not specified"))
                    .and_then(move |dbaddr| {
                        let sync_queue =
                            Arc::new(SqlBlobstoreSyncQueue::with_xdb(dbaddr, myrouter_port)?);
                        Ok(sync_queue)
                    })
                    .into_future()
            };

            let components: Vec<_> = blobstores
                .iter()
                .map({
                    cloned!(dbconfig);
                    move |(blobstoreid, config)| {
                        cloned!(blobstoreid);
                        make_blobstore(repoid, config, &dbconfig, myrouter_port)
                            .map({ move |store| (blobstoreid, store) })
                    }
                })
                .collect();

            queue
                .and_then({
                    cloned!(scuba_table);
                    move |queue| {
                        future::join_all(components).map({
                            move |components| {
                                MultiplexedBlobstore::new(
                                    repoid,
                                    components,
                                    queue,
                                    scuba_table.map(|table| Arc::new(ScubaClient::new(table))),
                                )
                            }
                        })
                    }
                })
                .map(|store| Arc::new(store) as Arc<dyn Blobstore>)
                .boxify()
        }
    }
}

/// Used by tests
pub fn new_memblob_empty(
    logger: Option<Logger>,
    blobstore: Option<Arc<Blobstore>>,
) -> Result<BlobRepo> {
    Ok(BlobRepo::new(
        logger.unwrap_or(Logger::root(Discard {}.ignore_res(), o!())),
        Arc::new(SqlBookmarks::with_sqlite_in_memory()?),
        blobstore.unwrap_or_else(|| Arc::new(EagerMemblob::new())),
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

fn new_filenodes(
    db_address: &String,
    sharded_filenodes: Option<ShardedFilenodesParams>,
    myrouter_port: Option<u16>,
) -> Result<CachingFilenodes> {
    let (tier, filenodes) = match (sharded_filenodes, myrouter_port) {
        (
            Some(ShardedFilenodesParams {
                shard_map,
                shard_num,
            }),
            Some(port),
        ) => {
            let conn = SqlFilenodes::with_sharded_myrouter(&shard_map, port, shard_num.into())?;
            (shard_map, conn)
        }
        (
            Some(ShardedFilenodesParams {
                shard_map,
                shard_num,
            }),
            None,
        ) => {
            let conn = SqlFilenodes::with_sharded_raw_xdb(&shard_map, shard_num.into())?;
            (shard_map, conn)
        }
        (None, port) => {
            let conn = SqlFilenodes::with_xdb(&db_address, port)?;
            (db_address.clone(), conn)
        }
    };

    let filenodes = CachingFilenodes::new(
        Arc::new(filenodes),
        cachelib::get_volatile_pool("filenodes")?.ok_or(Error::from(
            ErrorKind::MissingCachePool("filenodes".to_string()),
        ))?,
        "sqlfilenodes",
        &tier,
    );

    Ok(filenodes)
}

/// Create a new BlobRepo with purely local state. (Well, it could be a remote blobstore, but
/// that would be weird to use with a local metadata db.)
fn new_local(
    logger: Logger,
    dbpath: &Path,
    blobstore: Arc<Blobstore>,
    repoid: RepositoryId,
) -> Result<BlobRepo> {
    let bookmarks = SqlBookmarks::with_sqlite_path(dbpath.join("bookmarks"))
        .chain_err(ErrorKind::StateOpen(StateOpenError::Bookmarks))?;
    let filenodes = SqlFilenodes::with_sqlite_path(dbpath.join("filenodes"))
        .chain_err(ErrorKind::StateOpen(StateOpenError::Filenodes))?;
    let changesets = SqlChangesets::with_sqlite_path(dbpath.join("changesets"))
        .chain_err(ErrorKind::StateOpen(StateOpenError::Changesets))?;
    let bonsai_hg_mapping = SqlBonsaiHgMapping::with_sqlite_path(dbpath.join("bonsai_hg_mapping"))
        .chain_err(ErrorKind::StateOpen(StateOpenError::BonsaiHgMapping))?;

    Ok(BlobRepo::new(
        logger,
        Arc::new(bookmarks),
        blobstore,
        Arc::new(filenodes),
        Arc::new(changesets),
        Arc::new(bonsai_hg_mapping),
        repoid,
        Arc::new(DummyLease {}),
    ))
}

fn open_xdb<T: SqlConstructors>(addr: &str, myrouter_port: Option<u16>) -> Result<Arc<T>> {
    let ret = T::with_xdb(addr, myrouter_port)?;
    Ok(Arc::new(ret))
}

/// If the DB is remote then set up for a full production configuration.
/// In theory this could be with a local blobstore, but that would just be weird.
fn new_remote(
    logger: Logger,
    db_address: String,
    sharded_filenodes: Option<ShardedFilenodesParams>,
    blobstore: Arc<Blobstore>,
    repoid: RepositoryId,
    myrouter_port: Option<u16>,
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

    let filenodes = new_filenodes(&db_address, sharded_filenodes, myrouter_port)?;

    let bookmarks: Arc<dyn Bookmarks> = {
        let bookmarks = open_xdb::<SqlBookmarks>(&db_address, myrouter_port)?;
        if let Some(ttl) = bookmarks_cache_ttl {
            Arc::new(CachedBookmarks::new(bookmarks, ttl))
        } else {
            bookmarks
        }
    };

    let changesets = open_xdb::<SqlChangesets>(&db_address, myrouter_port)?;
    let changesets_cache_pool = cachelib::get_volatile_pool("changesets")?.ok_or(Error::from(
        ErrorKind::MissingCachePool("changesets".to_string()),
    ))?;
    let changesets = CachingChangesets::new(changesets, changesets_cache_pool.clone());
    let changesets = Arc::new(changesets);

    let bonsai_hg_mapping = open_xdb::<SqlBonsaiHgMapping>(&db_address, myrouter_port)?;
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
        Arc::new(filenodes),
        changesets,
        Arc::new(bonsai_hg_mapping),
        repoid,
        Arc::new(changeset_fetcher_factory),
        Arc::new(MemcacheOps::new("bonsai-hg-generation", "")?),
    ))
}
