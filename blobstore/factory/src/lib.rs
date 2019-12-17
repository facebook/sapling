/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::{path::PathBuf, sync::Arc};

use anyhow::{format_err, Error};
use cloned::cloned;
use failure_ext::chain::ChainExt;
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
use readonlyblob::ReadOnlyBlobstore;
use rocksblob::Rocksblob;
use scuba::ScubaSampleBuilder;
use slog::Logger;
use sql_ext::{
    create_myrouter_connections, create_raw_xdb_connections, create_sqlite_connections,
    myrouter_ready, MysqlOptions, PoolSizeConfig, SqlConnections,
};
use sqlblob::Sqlblob;
use sqlfilenodes::{SqlConstructors, SqlFilenodes};

#[derive(Copy, Clone, PartialEq)]
pub struct ReadOnlyStorage(pub bool);

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
    readonly: bool,
    mysql_options: MysqlOptions,
    sharded_filenodes: Option<ShardedFilenodesParams>,
}

impl XdbFactory {
    fn new(
        db_address: String,
        mysql_options: MysqlOptions,
        sharded_filenodes: Option<ShardedFilenodesParams>,
        readonly: bool,
    ) -> Self {
        XdbFactory {
            db_address,
            readonly,
            mysql_options,
            sharded_filenodes,
        }
    }
}

impl SqlFactoryBase for XdbFactory {
    fn open<T: SqlConstructors>(&self) -> BoxFuture<Arc<T>, Error> {
        T::with_xdb(self.db_address.clone(), self.mysql_options, self.readonly)
            .map(|r| Arc::new(r))
            .boxify()
    }

    fn open_filenodes(&self) -> BoxFuture<(String, Arc<SqlFilenodes>), Error> {
        let (tier, filenodes) = match self.sharded_filenodes.clone() {
            Some(ShardedFilenodesParams {
                shard_map,
                shard_num,
            }) => {
                let conn = SqlFilenodes::with_sharded_xdb(
                    shard_map.clone(),
                    self.mysql_options,
                    shard_num.into(),
                    self.readonly,
                );
                (shard_map, conn)
            }
            None => {
                let conn = SqlFilenodes::with_xdb(
                    self.db_address.clone(),
                    self.mysql_options,
                    self.readonly,
                );
                (self.db_address.clone(), conn)
            }
        };

        filenodes
            .map(move |filenodes| (tier, Arc::new(filenodes)))
            .boxify()
    }

    fn create_connections(&self, label: String) -> BoxFuture<SqlConnections, Error> {
        match self.mysql_options.myrouter_port {
            Some(mysql_options) => future::ok(create_myrouter_connections(
                self.db_address.clone(),
                None,
                mysql_options,
                self.mysql_options.myrouter_read_service_type(),
                PoolSizeConfig::for_regular_connection(),
                label,
                self.readonly,
            ))
            .boxify(),
            None => create_raw_xdb_connections(
                self.db_address.clone(),
                self.mysql_options.db_locator_read_instance_requirement(),
                self.readonly,
            )
            .boxify(),
        }
    }
}

struct SqliteFactory {
    path: PathBuf,
    readonly: bool,
}

impl SqliteFactory {
    fn new(path: PathBuf, readonly: bool) -> Self {
        SqliteFactory { path, readonly }
    }
}

impl SqlFactoryBase for SqliteFactory {
    fn open<T: SqlConstructors>(&self) -> BoxFuture<Arc<T>, Error> {
        let r = try_boxfuture!(T::with_sqlite_path(
            self.path.join("sqlite_dbs"),
            self.readonly
        ));
        Ok(Arc::new(r)).into_future().boxify()
    }

    fn open_filenodes(&self) -> BoxFuture<(String, Arc<SqlFilenodes>), Error> {
        self.open::<SqlFilenodes>()
            .map(|filenodes| ("sqlite".to_string(), filenodes))
            .boxify()
    }

    fn create_connections(&self, _label: String) -> BoxFuture<SqlConnections, Error> {
        create_sqlite_connections(&self.path.join("sqlite_dbs"), self.readonly)
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
    mysql_options: MysqlOptions,
    readonly: ReadOnlyStorage,
    logger: Logger,
) -> impl Future<Item = SqlFactory, Error = Error> {
    match dbconfig {
        MetadataDBConfig::LocalDB { path } => {
            let sql_factory = SqliteFactory::new(path.to_path_buf(), readonly.0);
            future::ok(SqlFactory {
                underlying: Either::Left(sql_factory),
            })
            .left_future()
        }
        MetadataDBConfig::Mysql {
            db_address,
            sharded_filenodes,
        } => {
            let sql_factory = XdbFactory::new(
                db_address.clone(),
                mysql_options,
                sharded_filenodes,
                readonly.0,
            );
            myrouter_ready(Some(db_address), mysql_options, logger)
                .map(move |()| SqlFactory {
                    underlying: Either::Right(sql_factory),
                })
                .right_future()
        }
    }
}

/// Constructs a blobstore, returns an error if blobstore type requires a mysql
pub fn make_blobstore_no_sql(
    fb: FacebookInit,
    blobconfig: &BlobConfig,
    readonly_storage: ReadOnlyStorage,
) -> BoxFuture<Arc<dyn Blobstore>, Error> {
    make_blobstore_impl(
        fb,
        blobconfig,
        None,
        MysqlOptions::default(),
        readonly_storage,
    )
}

/// Construct a blobstore according to the specification. The multiplexed blobstore
/// needs an SQL DB for its queue, as does the MySQL blobstore.
pub fn make_blobstore(
    fb: FacebookInit,
    blobconfig: &BlobConfig,
    sql_factory: &SqlFactory,
    mysql_options: MysqlOptions,
    readonly_storage: ReadOnlyStorage,
) -> BoxFuture<Arc<dyn Blobstore>, Error> {
    make_blobstore_impl(
        fb,
        blobconfig,
        Some(sql_factory),
        mysql_options,
        readonly_storage,
    )
}

fn make_blobstore_impl(
    fb: FacebookInit,
    blobconfig: &BlobConfig,
    sql_factory: Option<&SqlFactory>,
    mysql_options: MysqlOptions,
    readonly_storage: ReadOnlyStorage,
) -> BoxFuture<Arc<dyn Blobstore>, Error> {
    use BlobConfig::*;

    let read_write = match blobconfig {
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

        Sqlite { path } => Sqlblob::with_sqlite_path(path.join("blobs"), readonly_storage.0)
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
        } => if let Some(myrouter_port) = mysql_options.myrouter_port {
            Sqlblob::with_myrouter(
                fb,
                shard_map.clone(),
                myrouter_port,
                mysql_options.myrouter_read_service_type(),
                *shard_num,
                readonly_storage.0,
            )
        } else {
            Sqlblob::with_raw_xdb_shardmap(
                fb,
                shard_map.clone(),
                mysql_options.db_locator_read_instance_requirement(),
                *shard_num,
                readonly_storage.0,
            )
        }
        .map(|store| Arc::new(store) as Arc<dyn Blobstore>)
        .into_future()
        .boxify(),

        Multiplexed {
            scuba_table,
            blobstores,
        } => {
            let sql_factory = match sql_factory {
                Some(sql_factory) => sql_factory,
                None => {
                    let err = format_err!(
                        "sql factory is not specified, but multiplexed blobstore requires it",
                    );
                    return future::err(err).boxify();
                }
            };
            let queue = sql_factory.open::<SqlBlobstoreSyncQueue>();
            let components: Vec<_> = blobstores
                .iter()
                .map({
                    move |(blobstoreid, config)| {
                        cloned!(blobstoreid);
                        make_blobstore(fb, config, sql_factory, mysql_options, readonly_storage)
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
            let sql_factory = match sql_factory {
                Some(sql_factory) => sql_factory,
                None => {
                    let err = format_err!(
                        "sql factory is not specified, but scrub blobstore requires it"
                    );
                    return future::err(err).boxify();
                }
            };
            let queue = sql_factory.open::<SqlBlobstoreSyncQueue>();
            let components: Vec<_> = blobstores
                .iter()
                .map({
                    move |(blobstoreid, config)| {
                        cloned!(blobstoreid);
                        make_blobstore(fb, config, sql_factory, mysql_options, readonly_storage)
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
    };

    if readonly_storage.0 {
        read_write
            .map(|inner| Arc::new(ReadOnlyBlobstore::new(inner)) as Arc<dyn Blobstore>)
            .boxify()
    } else {
        read_write
    }
}
