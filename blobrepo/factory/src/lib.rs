// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobrepo::BlobRepo;
use blobrepo_errors::*;
use cloned::cloned;
use dbbookmarks::SqlBookmarks;
use failure_ext::{err_msg, Error, Result};
use futures::{
    future::{self, IntoFuture},
    Future,
};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use metaconfig_types::{self, RepoType};
use mononoke_types::RepositoryId;
use slog::{self, o, Discard, Drain, Logger};
use sqlfilenodes::{SqlConstructors, SqlFilenodes};
use std::path::Path;
use std::sync::Arc;

use blobstore::Blobstore;
use blobstore_sync_queue::{BlobstoreSyncQueue, SqlBlobstoreSyncQueue};
use bonsai_hg_mapping::{CachingBonsaiHgMapping, SqlBonsaiHgMapping};
use cacheblob::{new_cachelib_blobstore, new_memcache_blobstore};
use changeset_fetcher::{ChangesetFetcher, SimpleChangesetFetcher};
use changesets::{CachingChangests, SqlChangesets};
use filenodes::CachingFilenodes;
use memblob::EagerMemblob;
use prefixblob::PrefixBlobstore;

use fileblob::Fileblob;
use sqlblob::Sqlblob;
use std::time::Duration;

use delayblob::DelayBlob;
use failure_ext::prelude::*;
use glusterblob::Glusterblob;
use manifoldblob::ThriftManifoldBlob;
use metaconfig_types::RemoteBlobstoreArgs;
use multiplexedblob::MultiplexedBlobstore;
use rocksblob::Rocksblob;
use rocksdb;
use scuba::ScubaClient;

/// Create a new BlobRepo with purely local state.
pub fn new_local(
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
    ))
}

/// Most local use cases should use new_rocksdb instead. This is only meant for test
/// fixtures.
pub fn new_files(logger: Logger, path: &Path, repoid: RepositoryId) -> Result<BlobRepo> {
    let blobstore = Fileblob::create(path.join("blobs"))
        .chain_err(ErrorKind::StateOpen(StateOpenError::Blobstore))?;
    new_local(logger, path, Arc::new(blobstore), repoid)
}

pub fn new_rocksdb(logger: Logger, path: &Path, repoid: RepositoryId) -> Result<BlobRepo> {
    let options = rocksdb::Options::new().create_if_missing(true);
    let blobstore = Rocksblob::open_with_options(path.join("blobs"), options)
        .chain_err(ErrorKind::StateOpen(StateOpenError::Blobstore))?;
    new_local(logger, path, Arc::new(blobstore), repoid)
}

pub fn new_rocksdb_delayed<F>(
    logger: Logger,
    path: &Path,
    repoid: RepositoryId,
    delay_gen: F,
    get_roundtrips: usize,
    put_roundtrips: usize,
    is_present_roundtrips: usize,
    assert_present_roundtrips: usize,
) -> Result<BlobRepo>
where
    F: FnMut(()) -> Duration + 'static + Send + Sync,
{
    let options = rocksdb::Options::new().create_if_missing(true);
    let blobstore = Rocksblob::open_with_options(path.join("blobs"), options)
        .chain_err(ErrorKind::StateOpen(StateOpenError::Blobstore))?;
    let blobstore = DelayBlob::new(
        Box::new(blobstore),
        delay_gen,
        get_roundtrips,
        put_roundtrips,
        is_present_roundtrips,
        assert_present_roundtrips,
    );
    new_local(logger, path, Arc::new(blobstore), repoid)
}

pub fn new_sqlite(logger: Logger, path: &Path, repoid: RepositoryId) -> Result<BlobRepo> {
    let blobstore = Sqlblob::with_sqlite_path(repoid, path.join("blobs"))
        .chain_err(ErrorKind::StateOpen(StateOpenError::Blobstore))?;
    new_local(logger, path, Arc::new(blobstore), repoid)
}

pub fn open_blobrepo(
    logger: slog::Logger,
    repotype: RepoType,
    repoid: RepositoryId,
    myrouter_port: Option<u16>,
) -> impl Future<Item = BlobRepo, Error = Error> {
    use metaconfig_types::RepoType::*;

    match repotype {
        BlobFiles(ref path) => new_files(logger, &path, repoid)
            .into_future()
            .left_future(),
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
            ref filenode_shards,
        } => {
            let myrouter_port = match myrouter_port {
                None => {
                    return future::err(err_msg(
                        "Missing myrouter port, unable to open BlobRemote repo",
                    ))
                    .left_future();
                }
                Some(myrouter_port) => myrouter_port,
            };
            new_remote(
                logger,
                blobstores_args,
                db_address.clone(),
                filenode_shards.clone(),
                repoid,
                myrouter_port,
            )
            .right_future()
        }
    }
}

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
    ))
}

pub fn new_remote(
    logger: Logger,
    args: &RemoteBlobstoreArgs,
    db_address: String,
    filenode_shards: Option<usize>,
    repoid: RepositoryId,
    myrouter_port: u16,
) -> impl Future<Item = BlobRepo, Error = Error> {
    // recursively construct blobstore from arguments
    fn eval_remote_args(
        args: RemoteBlobstoreArgs,
        repoid: RepositoryId,
        myrouter_port: u16,
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
                let blobstore: Arc<Blobstore> = Arc::new(Sqlblob::with_myrouter(
                    repoid,
                    args.shardmap,
                    myrouter_port,
                    args.shard_num,
                ));
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

    let blobstore_sync_queue: Arc<BlobstoreSyncQueue> = Arc::new(
        SqlBlobstoreSyncQueue::with_myrouter(&db_address, myrouter_port),
    );
    eval_remote_args(args.clone(), repoid, myrouter_port, blobstore_sync_queue).and_then(
        move |blobstore| {
            let blobstore = new_memcache_blobstore(blobstore, "multiplexed", "")?;
            let blob_pool = Arc::new(cachelib::get_pool("blobstore-blobs").ok_or(Error::from(
                ErrorKind::MissingCachePool("blobstore-blobs".to_string()),
            ))?);
            let presence_pool =
                Arc::new(cachelib::get_pool("blobstore-presence").ok_or(Error::from(
                    ErrorKind::MissingCachePool("blobstore-presence".to_string()),
                ))?);
            let blobstore = Arc::new(new_cachelib_blobstore(blobstore, blob_pool, presence_pool));

            let filenodes = match filenode_shards {
                Some(shards) => {
                    SqlFilenodes::with_sharded_myrouter(&db_address, myrouter_port, shards)
                }
                None => SqlFilenodes::with_myrouter(&db_address, myrouter_port),
            };
            let filenodes = CachingFilenodes::new(
                Arc::new(filenodes),
                cachelib::get_pool("filenodes").ok_or(Error::from(ErrorKind::MissingCachePool(
                    "filenodes".to_string(),
                )))?,
                "sqlfilenodes",
                &db_address,
            );

            let bookmarks = SqlBookmarks::with_myrouter(&db_address, myrouter_port);

            let changesets = SqlChangesets::with_myrouter(&db_address, myrouter_port);
            let changesets_cache_pool = cachelib::get_pool("changesets").ok_or(Error::from(
                ErrorKind::MissingCachePool("changesets".to_string()),
            ))?;
            let changesets =
                CachingChangests::new(Arc::new(changesets), changesets_cache_pool.clone());
            let changesets = Arc::new(changesets);

            let bonsai_hg_mapping = SqlBonsaiHgMapping::with_myrouter(&db_address, myrouter_port);
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
                Arc::new(bookmarks),
                blobstore,
                Arc::new(filenodes),
                changesets,
                Arc::new(bonsai_hg_mapping),
                repoid,
                Arc::new(changeset_fetcher_factory),
            ))
        },
    )
}
