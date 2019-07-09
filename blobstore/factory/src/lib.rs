// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{path::PathBuf, sync::Arc};

use cloned::cloned;
use failure_ext::prelude::*;
use failure_ext::{Error, Result};
use futures::{
    future::{self, IntoFuture},
    Future,
};
use futures_ext::{BoxFuture, FutureExt};

use blobstore::ErrorKind;
use blobstore::{Blobstore, DisabledBlob};
use blobstore_sync_queue::SqlBlobstoreSyncQueue;
use censoredblob::CensoredBlob;
use fileblob::Fileblob;
use glusterblob::Glusterblob;
use manifoldblob::ThriftManifoldBlob;
use metaconfig_types::{self, BlobConfig, ShardedFilenodesParams};
use mononoke_types::RepositoryId;
use multiplexedblob::{MultiplexedBlobstore, ScrubBlobstore};
use prefixblob::PrefixBlobstore;
use rocksblob::Rocksblob;
use rocksdb;
use scuba::ScubaClient;
use sqlblob::Sqlblob;
use sqlfilenodes::{SqlConstructors, SqlFilenodes};
use std::collections::HashMap;

#[derive(Copy, Clone, PartialEq)]
pub enum Scrubbing {
    Enabled,
    Disabled,
}

pub trait SqlFactory: Send + Sync {
    /// Open an arbitrary struct implementing SqlConstructors
    fn open<T: SqlConstructors>(&self) -> Result<Arc<T>>;

    /// Open SqlFilenodes, and return a tier name and the struct.
    fn open_filenodes(&self) -> Result<(String, Arc<SqlFilenodes>)>;
}

pub struct XdbFactory {
    db_address: String,
    myrouter_port: Option<u16>,
    sharded_filenodes: Option<ShardedFilenodesParams>,
}

impl XdbFactory {
    pub fn new(
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

pub struct SqliteFactory {
    path: PathBuf,
}

impl SqliteFactory {
    pub fn new(path: PathBuf) -> Self {
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

/// Construct a blobstore according to the specification. The multiplexed blobstore
/// needs an SQL DB for its queue, as does the MySQL blobstore.
pub fn make_blobstore<T: SqlFactory>(
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

        Sqlite { path } => Sqlblob::with_sqlite_path(repoid, path.join("blobs"))
            .chain_err(ErrorKind::StateOpen)
            .map_err(Error::from)
            .map(|store| Arc::new(store) as Arc<dyn Blobstore>)
            .into_future()
            .boxify(),

        Manifold { bucket, prefix } => ThriftManifoldBlob::new(bucket.clone())
            .map({
                cloned!(prefix);
                move |manifold| PrefixBlobstore::new(manifold, format!("flat/{}", prefix))
            })
            .chain_err(ErrorKind::StateOpen)
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

pub fn make_censored_prefixed_blobstore<T: Blobstore + Clone>(
    inner_blobstore: T,
    censored_blobs: Option<HashMap<String, String>>,
    prefix: String,
) -> CensoredBlob<PrefixBlobstore<T>> {
    CensoredBlob::new(
        PrefixBlobstore::new(inner_blobstore, prefix),
        censored_blobs,
    )
}
