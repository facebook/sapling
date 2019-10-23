/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::{path::PathBuf, sync::Arc};

use cloned::cloned;
use failure_ext::prelude::*;
use failure_ext::Error;
use fbinit::FacebookInit;
use futures::{
    future::{self, IntoFuture},
    Future,
};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};

use blobstore::ErrorKind;
use blobstore::{Blobstore, DisabledBlob};
use blobstore_sync_queue::SqlBlobstoreSyncQueue;
use fileblob::Fileblob;
use itertools::Either;
use manifoldblob::ThriftManifoldBlob;
use metaconfig_types::{self, BlobConfig, MetadataDBConfig, ShardedFilenodesParams};
use multiplexedblob::{MultiplexedBlobstore, ScrubBlobstore};
use prefixblob::PrefixBlobstore;
use rocksblob::Rocksblob;
use scuba::ScubaSampleBuilder;
use slog::Logger;
use sql_ext::{
    create_myrouter_connections, create_raw_xdb_connections, create_sqlite_connections,
    myrouter_ready, PoolSizeConfig, SqlConnections,
};
use sqlblob::Sqlblob;
use sqlfilenodes::{SqlConstructors, SqlFilenodes};

#[derive(Copy, Clone, PartialEq)]
pub enum Scrubbing {
    Enabled,
    Disabled,
}

trait SqlFactoryBase: Send + Sync {
    /// Open an arbitrary struct implementing SqlConstructors
    fn open<T: SqlConstructors>(&self) -> BoxFuture<Arc<T>, Error>;

    /// Open SqlFilenodes, and return a tier name and the struct.
    fn open_filenodes(&self) -> BoxFuture<(String, Arc<SqlFilenodes>), Error>;

    /// Creates connections to the db.
    fn create_connections(&self, label: String) -> BoxFuture<SqlConnections, Error>;
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

impl SqlFactoryBase for XdbFactory {
    fn open<T: SqlConstructors>(&self) -> BoxFuture<Arc<T>, Error> {
        T::with_xdb(self.db_address.clone(), self.myrouter_port)
            .map(|r| Arc::new(r))
            .boxify()
    }

    fn open_filenodes(&self) -> BoxFuture<(String, Arc<SqlFilenodes>), Error> {
        let (tier, filenodes) = match (self.sharded_filenodes.clone(), self.myrouter_port) {
            (
                Some(ShardedFilenodesParams {
                    shard_map,
                    shard_num,
                }),
                Some(port),
            ) => {
                let conn =
                    SqlFilenodes::with_sharded_myrouter(shard_map.clone(), port, shard_num.into());
                (shard_map, conn)
            }
            (
                Some(ShardedFilenodesParams {
                    shard_map,
                    shard_num,
                }),
                None,
            ) => {
                let conn = SqlFilenodes::with_sharded_raw_xdb(shard_map.clone(), shard_num.into());
                (shard_map, conn)
            }
            (None, port) => {
                let conn = SqlFilenodes::with_xdb(self.db_address.clone(), port);
                (self.db_address.clone(), conn)
            }
        };

        filenodes
            .map(move |filenodes| (tier, Arc::new(filenodes)))
            .boxify()
    }

    fn create_connections(&self, label: String) -> BoxFuture<SqlConnections, Error> {
        match self.myrouter_port {
            Some(myrouter_port) => future::ok(create_myrouter_connections(
                self.db_address.clone(),
                None,
                myrouter_port,
                PoolSizeConfig::for_regular_connection(),
                label,
            ))
            .boxify(),
            None => create_raw_xdb_connections(self.db_address.clone()).boxify(),
        }
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

impl SqlFactoryBase for SqliteFactory {
    fn open<T: SqlConstructors>(&self) -> BoxFuture<Arc<T>, Error> {
        let r = try_boxfuture!(T::with_sqlite_path(self.path.join(T::LABEL)));
        Ok(Arc::new(r)).into_future().boxify()
    }

    fn open_filenodes(&self) -> BoxFuture<(String, Arc<SqlFilenodes>), Error> {
        self.open::<SqlFilenodes>()
            .map(|filenodes| ("sqlite".to_string(), filenodes))
            .boxify()
    }

    fn create_connections(&self, label: String) -> BoxFuture<SqlConnections, Error> {
        create_sqlite_connections(&self.path.join(label))
            .into_future()
            .boxify()
    }
}

pub struct SqlFactory {
    underlying: Either<SqliteFactory, XdbFactory>,
}

impl SqlFactory {
    pub fn open<T: SqlConstructors>(&self) -> BoxFuture<Arc<T>, Error> {
        self.underlying.as_ref().either(|l| l.open(), |r| r.open())
    }

    pub fn open_filenodes(&self) -> BoxFuture<(String, Arc<SqlFilenodes>), Error> {
        self.underlying
            .as_ref()
            .either(|l| l.open_filenodes(), |r| r.open_filenodes())
    }

    pub fn create_connections(&self, label: String) -> BoxFuture<SqlConnections, Error> {
        self.underlying.as_ref().either(
            {
                cloned!(label);
                move |l| l.create_connections(label)
            },
            |r| r.create_connections(label),
        )
    }
}

pub fn make_sql_factory(
    dbconfig: MetadataDBConfig,
    myrouter_port: Option<u16>,
    logger: Logger,
) -> impl Future<Item = SqlFactory, Error = Error> {
    match dbconfig {
        MetadataDBConfig::LocalDB { path } => {
            let sql_factory = SqliteFactory::new(path.to_path_buf());
            future::ok(SqlFactory {
                underlying: Either::Left(sql_factory),
            })
            .left_future()
        }
        MetadataDBConfig::Mysql {
            db_address,
            sharded_filenodes,
        } => {
            let sql_factory = XdbFactory::new(db_address.clone(), myrouter_port, sharded_filenodes);
            myrouter_ready(Some(db_address), myrouter_port, logger)
                .map(move |()| SqlFactory {
                    underlying: Either::Right(sql_factory),
                })
                .right_future()
        }
    }
}

/// Construct a blobstore according to the specification. The multiplexed blobstore
/// needs an SQL DB for its queue, as does the MySQL blobstore.
pub fn make_blobstore(
    fb: FacebookInit,
    blobconfig: &BlobConfig,
    sql_factory: &SqlFactory,
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
            .chain_err(ErrorKind::StateOpen)
            .map(|store| Arc::new(store) as Arc<dyn Blobstore>)
            .map_err(Error::from)
            .into_future()
            .boxify(),

        Rocks { path } => {
            let options = rocksdb::Options::new().create_if_missing(true);
            Rocksblob::open_with_options(path.join("blobs"), options)
                .chain_err(ErrorKind::StateOpen)
                .map(|store| Arc::new(store) as Arc<dyn Blobstore>)
                .map_err(Error::from)
                .into_future()
                .boxify()
        }

        Sqlite { path } => Sqlblob::with_sqlite_path(path.join("blobs"))
            .chain_err(ErrorKind::StateOpen)
            .map_err(Error::from)
            .map(|store| Arc::new(store) as Arc<dyn Blobstore>)
            .into_future()
            .boxify(),

        Manifold { bucket, prefix } => ThriftManifoldBlob::new(fb, bucket.clone())
            .map({
                cloned!(prefix);
                move |manifold| PrefixBlobstore::new(manifold, format!("flat/{}", prefix))
            })
            .chain_err(ErrorKind::StateOpen)
            .map_err(Error::from)
            .map(|store| Arc::new(store) as Arc<dyn Blobstore>)
            .into_future()
            .boxify(),

        Mysql {
            shard_map,
            shard_num,
        } => if let Some(myrouter_port) = myrouter_port {
            Sqlblob::with_myrouter(fb, shard_map.clone(), myrouter_port, *shard_num)
        } else {
            Sqlblob::with_raw_xdb_shardmap(fb, shard_map.clone(), *shard_num)
        }
        .map(|store| Arc::new(store) as Arc<dyn Blobstore>)
        .into_future()
        .boxify(),

        Multiplexed {
            scuba_table,
            blobstores,
        } => {
            let queue = sql_factory.open::<SqlBlobstoreSyncQueue>();
            let components: Vec<_> = blobstores
                .iter()
                .map({
                    move |(blobstoreid, config)| {
                        cloned!(blobstoreid);
                        make_blobstore(fb, config, sql_factory, myrouter_port)
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
                                Arc::new(MultiplexedBlobstore::new(
                                    components,
                                    queue,
                                    scuba_table
                                        .map_or(ScubaSampleBuilder::with_discard(), |table| {
                                            ScubaSampleBuilder::new(fb, table)
                                        }),
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
            let queue = sql_factory.open::<SqlBlobstoreSyncQueue>();
            let components: Vec<_> = blobstores
                .iter()
                .map({
                    move |(blobstoreid, config)| {
                        cloned!(blobstoreid);
                        make_blobstore(fb, config, sql_factory, myrouter_port)
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
                                    components,
                                    queue,
                                    scuba_table
                                        .map_or(ScubaSampleBuilder::with_discard(), |table| {
                                            ScubaSampleBuilder::new(fb, table)
                                        }),
                                )) as Arc<dyn Blobstore>
                            }
                        })
                    }
                })
                .boxify()
        }

        ManifoldWithTtl {
            bucket,
            prefix,
            ttl,
        } => ThriftManifoldBlob::new_with_ttl(fb, bucket.clone(), *ttl)
            .map({
                cloned!(prefix);
                move |manifold| PrefixBlobstore::new(manifold, format!("flat/{}", prefix))
            })
            .chain_err(ErrorKind::StateOpen)
            .map_err(Error::from)
            .map(|store| Arc::new(store) as Arc<dyn Blobstore>)
            .into_future()
            .boxify(),
    }
}
