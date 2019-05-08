// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobrepo::BlobRepo;
use blobrepo_errors::*;
use blobstore::Blobstore;
use blobstore_sync_queue::{BlobstoreSyncQueue, SqlBlobstoreSyncQueue};
use bonsai_hg_mapping::{CachingBonsaiHgMapping, SqlBonsaiHgMapping};
use bookmarks::{Bookmarks, CachedBookmarks};
use cacheblob::{dummy::DummyLease, new_cachelib_blobstore, new_memcache_blobstore, MemcacheOps};
use changeset_fetcher::{ChangesetFetcher, SimpleChangesetFetcher};
use changesets::{CachingChangests, SqlChangesets};
use cloned::cloned;
use dbbookmarks::SqlBookmarks;
use failure_ext::prelude::*;
use failure_ext::{Error, Result};
use fileblob::Fileblob;
use filenodes::CachingFilenodes;
use futures::{
    future::{self, IntoFuture},
    Future,
};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use glusterblob::Glusterblob;
use manifoldblob::ThriftManifoldBlob;
use memblob::EagerMemblob;
use metaconfig_types::{self, RemoteBlobstoreArgs, RepoType, ShardedFilenodesParams};
use mononoke_types::RepositoryId;
use multiplexedblob::MultiplexedBlobstore;
use prefixblob::PrefixBlobstore;
use rocksblob::Rocksblob;
use rocksdb;
use scuba::ScubaClient;
use slog::{self, o, Discard, Drain, Logger};
use sqlblob::Sqlblob;
use sqlfilenodes::{SqlConstructors, SqlFilenodes};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// Create a new BlobRepo with purely local state.
fn new_local(
    logger: Logger,
    path: &Path,
    blobstore: Arc<Blobstore>,
    repoid: RepositoryId,
) -> Result<BlobRepo> {
    let bookmarks = SqlBookmarks::with_sqlite_path(path.join("books"))
        .chain_err(ErrorKind::StateOpen(StateOpenError::Bookmarks))?;
    let filenodes = SqlFilenodes::with_sqlite_path(path.join("filenodes"))
        .chain_err(ErrorKind::StateOpen(StateOpenError::Filenodes))?;
    let changesets = SqlChangesets::with_sqlite_path(path.join("changesets"))
        .chain_err(ErrorKind::StateOpen(StateOpenError::Changesets))?;
    let bonsai_hg_mapping = SqlBonsaiHgMapping::with_sqlite_path(path.join("bonsai_hg_mapping"))
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

/// Most local use cases should use new_rocksdb instead. This is only meant for test
/// fixtures.
fn new_files(logger: Logger, path: &Path, repoid: RepositoryId) -> Result<BlobRepo> {
    let blobstore = Fileblob::create(path.join("blobs"))
        .chain_err(ErrorKind::StateOpen(StateOpenError::Blobstore))?;
    new_local(logger, path, Arc::new(blobstore), repoid)
}

fn new_rocksdb(logger: Logger, path: &Path, repoid: RepositoryId) -> Result<BlobRepo> {
    let options = rocksdb::Options::new().create_if_missing(true);
    let blobstore = Rocksblob::open_with_options(path.join("blobs"), options)
        .chain_err(ErrorKind::StateOpen(StateOpenError::Blobstore))?;
    new_local(logger, path, Arc::new(blobstore), repoid)
}

fn new_sqlite(logger: Logger, path: &Path, repoid: RepositoryId) -> Result<BlobRepo> {
    let blobstore = Sqlblob::with_sqlite_path(repoid, path.join("blobs"))
        .chain_err(ErrorKind::StateOpen(StateOpenError::Blobstore))?;
    new_local(logger, path, Arc::new(blobstore), repoid)
}

pub fn open_blobrepo(
    logger: slog::Logger,
    repotype: RepoType,
    repoid: RepositoryId,
    myrouter_port: Option<u16>,
    bookmarks_cache_ttl: Option<Duration>,
) -> impl Future<Item = BlobRepo, Error = Error> {
    use metaconfig_types::RepoType::*;

    match repotype {
        BlobFiles(ref path) => new_files(logger, &path, repoid).into_future().left_future(),
        BlobRocks(ref path) => new_rocksdb(logger, &path, repoid)
            .into_future()
            .left_future(),
        BlobSqlite(ref path) => new_sqlite(logger, &path, repoid)
            .into_future()
            .left_future(),
        BlobRemote {
            ref blobstores_args,
            ref db_address,
            write_lock_db_address: _,
            ref sharded_filenodes,
        } => new_remote(
            logger,
            blobstores_args,
            db_address.clone(),
            sharded_filenodes.clone(),
            repoid,
            myrouter_port,
            bookmarks_cache_ttl,
        )
        .right_future(),
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
        (None, Some(port)) => {
            let conn = SqlFilenodes::with_myrouter(&db_address, port);
            (db_address.clone(), conn)
        }
        (None, None) => {
            let conn = SqlFilenodes::with_raw_xdb_tier(&db_address)?;
            (db_address.clone(), conn)
        }
    };

    let filenodes = CachingFilenodes::new(
        Arc::new(filenodes),
        cachelib::get_pool("filenodes").ok_or(Error::from(ErrorKind::MissingCachePool(
            "filenodes".to_string(),
        )))?,
        "sqlfilenodes",
        &tier,
    );

    Ok(filenodes)
}

pub fn new_remote(
    logger: Logger,
    args: &RemoteBlobstoreArgs,
    db_address: String,
    sharded_filenodes: Option<ShardedFilenodesParams>,
    repoid: RepositoryId,
    myrouter_port: Option<u16>,
    bookmarks_cache_ttl: Option<Duration>,
) -> impl Future<Item = BlobRepo, Error = Error> {
    // recursively construct blobstore from arguments
    fn eval_remote_args(
        args: RemoteBlobstoreArgs,
        repoid: RepositoryId,
        myrouter_port: Option<u16>,
        queue: Arc<BlobstoreSyncQueue>,
    ) -> BoxFuture<Arc<Blobstore>, Error> {
        match args {
            RemoteBlobstoreArgs::Manifold(manifold_args) => {
                let blobstore: Arc<Blobstore> = Arc::new(PrefixBlobstore::new(
                    try_boxfuture!(ThriftManifoldBlob::new(manifold_args.bucket.clone())),
                    format!("flat/{}", manifold_args.prefix),
                ));
                future::ok(blobstore).boxify()
            }
            RemoteBlobstoreArgs::Gluster(args) => {
                Glusterblob::with_smc(args.tier, args.export, args.basepath)
                    .map(|blobstore| -> Arc<Blobstore> { Arc::new(blobstore) })
                    .boxify()
            }
            RemoteBlobstoreArgs::Mysql(args) => {
                let blobstore: Arc<Blobstore> = match myrouter_port {
                    Some(myrouter_port) => Arc::new(try_boxfuture!(Sqlblob::with_myrouter(
                        repoid,
                        args.shardmap,
                        myrouter_port,
                        args.shard_num,
                    ))),
                    None => Arc::new(try_boxfuture!(Sqlblob::with_raw_xdb_shardmap(
                        repoid,
                        args.shardmap,
                        args.shard_num,
                    ))),
                };
                future::ok(blobstore).boxify()
            }
            RemoteBlobstoreArgs::Multiplexed {
                scuba_table,
                blobstores,
            } => {
                let blobstores: Vec<_> = blobstores
                    .into_iter()
                    .map(|(blobstore_id, arg)| {
                        eval_remote_args(arg, repoid, myrouter_port, queue.clone())
                            .map(move |blobstore| (blobstore_id, blobstore))
                    })
                    .collect();
                future::join_all(blobstores)
                    .map(move |blobstores| {
                        if blobstores.len() == 1 {
                            let (_, blobstore) = blobstores.into_iter().next().unwrap();
                            blobstore
                        } else {
                            Arc::new(MultiplexedBlobstore::new(
                                repoid,
                                blobstores,
                                queue.clone(),
                                scuba_table.map(|table| Arc::new(ScubaClient::new(table))),
                            ))
                        }
                    })
                    .boxify()
            }
        }
    }

    let blobstore_sync_queue: Arc<BlobstoreSyncQueue> = match myrouter_port {
        Some(myrouter_port) => Arc::new(SqlBlobstoreSyncQueue::with_myrouter(
            &db_address,
            myrouter_port,
        )),
        None => Arc::new(try_boxfuture!(SqlBlobstoreSyncQueue::with_raw_xdb_tier(
            &db_address
        ))),
    };
    eval_remote_args(args.clone(), repoid, myrouter_port, blobstore_sync_queue)
        .and_then(move |blobstore| {
            let blobstore = new_memcache_blobstore(blobstore, "multiplexed", "")?;
            let blob_pool = Arc::new(cachelib::get_pool("blobstore-blobs").ok_or(Error::from(
                ErrorKind::MissingCachePool("blobstore-blobs".to_string()),
            ))?);
            let presence_pool =
                Arc::new(cachelib::get_pool("blobstore-presence").ok_or(Error::from(
                    ErrorKind::MissingCachePool("blobstore-presence".to_string()),
                ))?);
            let blobstore = Arc::new(new_cachelib_blobstore(blobstore, blob_pool, presence_pool));

            let filenodes = new_filenodes(&db_address, sharded_filenodes, myrouter_port)?;

            let bookmarks: Arc<dyn Bookmarks> = {
                let bookmarks = match myrouter_port {
                    Some(myrouter_port) => {
                        Arc::new(SqlBookmarks::with_myrouter(&db_address, myrouter_port))
                    }
                    None => Arc::new(SqlBookmarks::with_raw_xdb_tier(&db_address)?),
                };
                if let Some(ttl) = bookmarks_cache_ttl {
                    Arc::new(CachedBookmarks::new(bookmarks, ttl))
                } else {
                    bookmarks
                }
            };

            let changesets = match myrouter_port {
                Some(myrouter_port) => SqlChangesets::with_myrouter(&db_address, myrouter_port),
                None => SqlChangesets::with_raw_xdb_tier(&db_address)?,
            };
            let changesets_cache_pool = cachelib::get_pool("changesets").ok_or(Error::from(
                ErrorKind::MissingCachePool("changesets".to_string()),
            ))?;
            let changesets =
                CachingChangests::new(Arc::new(changesets), changesets_cache_pool.clone());
            let changesets = Arc::new(changesets);

            let bonsai_hg_mapping = match myrouter_port {
                Some(myrouter_port) => {
                    SqlBonsaiHgMapping::with_myrouter(&db_address, myrouter_port)
                }
                None => SqlBonsaiHgMapping::with_raw_xdb_tier(&db_address)?,
            };
            let bonsai_hg_mapping = CachingBonsaiHgMapping::new(
                Arc::new(bonsai_hg_mapping),
                cachelib::get_pool("bonsai_hg_mapping").ok_or(Error::from(
                    ErrorKind::MissingCachePool("bonsai_hg_mapping".to_string()),
                ))?,
            );

            let changeset_fetcher_factory = {
                cloned!(changesets, repoid);
                move || {
                    let res: Arc<ChangesetFetcher + Send + Sync> = Arc::new(
                        SimpleChangesetFetcher::new(changesets.clone(), repoid.clone()),
                    );
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
        })
        .boxify()
}
