/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

mod delay;
#[cfg(fbcode_build)]
mod facebook;
#[cfg(not(fbcode_build))]
mod myadmin_delay_dummy;
mod store;
#[cfg(test)]
mod tests;

use crate::delay::BlobDelay;
#[cfg(fbcode_build)]
use crate::facebook::myadmin_delay;
#[cfg(not(fbcode_build))]
use crate::myadmin_delay_dummy as myadmin_delay;
use crate::store::{ChunkSqlStore, ChunkingMethod, DataSqlStore};
use anyhow::{format_err, Error, Result};
use blobstore::{Blobstore, BlobstoreGetData, BlobstoreMetadata, CountedBlobstore};
use bytes::BytesMut;
use cloned::cloned;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::{
    future::{FutureExt, TryFutureExt},
    stream::{FuturesOrdered, TryStreamExt},
};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt as _};
use futures_old::future::join_all;
use futures_old::prelude::*;
use mononoke_types::{hash::Context as HashContext, BlobstoreBytes};
use sql::{rusqlite::Connection as SqliteConnection, Connection};
use sql_ext::{
    facebook::{
        create_myrouter_connections, create_raw_xdb_connections, PoolSizeConfig, ReadConnectionType,
    },
    open_sqlite_in_memory, open_sqlite_path, SqlConnections,
};
use std::convert::TryInto;
use std::fmt;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

// Leaving some space for metadata
const MAX_KEY_SIZE: usize = 200;
// MySQL wants multiple chunks, each around 1 MiB, as a tradeoff between query latency and replication lag
const CHUNK_SIZE: usize = 1024 * 1024;
const SQLITE_SHARD_NUM: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(2) };

const COUNTED_ID: &str = "sqlblob";
pub type CountedSqlblob = CountedBlobstore<Sqlblob>;

pub struct Sqlblob {
    data_store: Arc<DataSqlStore>,
    chunk_store: Arc<ChunkSqlStore>,
}

impl Sqlblob {
    pub fn with_myrouter(
        fb: FacebookInit,
        shardmap: String,
        port: u16,
        read_con_type: ReadConnectionType,
        shard_num: NonZeroUsize,
        readonly: bool,
    ) -> BoxFuture<CountedSqlblob, Error> {
        let delay = try_boxfuture!(myadmin_delay::sharded(fb, shardmap.clone(), shard_num));
        Self::with_connection_factory(delay, shardmap.clone(), shard_num, move |shard_id| {
            Ok(create_myrouter_connections(
                shardmap.clone(),
                Some(shard_id),
                port,
                read_con_type,
                PoolSizeConfig::for_sharded_connection(),
                "blobstore".into(),
                readonly,
            ))
            .into_future()
            .boxify()
        })
    }

    pub fn with_myrouter_unsharded(
        fb: FacebookInit,
        db_address: String,
        port: u16,
        read_con_type: ReadConnectionType,
        readonly: bool,
    ) -> BoxFuture<CountedSqlblob, Error> {
        let delay = try_boxfuture!(myadmin_delay::single(fb, db_address.clone()));
        Self::with_connection_factory(
            delay,
            db_address.clone(),
            NonZeroUsize::new(1).expect("One should be greater than zero"),
            move |_shard_id| {
                Ok(create_myrouter_connections(
                    db_address.clone(),
                    None,
                    port,
                    read_con_type,
                    PoolSizeConfig::for_sharded_connection(),
                    "blobstore".into(),
                    readonly,
                ))
                .into_future()
                .boxify()
            },
        )
    }

    pub fn with_raw_xdb_shardmap(
        fb: FacebookInit,
        shardmap: String,
        read_con_type: ReadConnectionType,
        shard_num: NonZeroUsize,
        readonly: bool,
    ) -> BoxFuture<CountedSqlblob, Error> {
        let delay = try_boxfuture!(myadmin_delay::sharded(fb, shardmap.clone(), shard_num));
        Self::with_connection_factory(delay, shardmap.clone(), shard_num, move |shard_id| {
            create_raw_xdb_connections(
                fb,
                format!("{}.{}", shardmap, shard_id),
                read_con_type,
                readonly,
            )
            .boxify()
        })
    }

    pub fn with_raw_xdb_unsharded(
        fb: FacebookInit,
        db_address: String,
        read_con_type: ReadConnectionType,
        readonly: bool,
    ) -> BoxFuture<CountedSqlblob, Error> {
        let delay = try_boxfuture!(myadmin_delay::single(fb, db_address.clone()));
        Self::with_connection_factory(
            delay,
            db_address.clone(),
            NonZeroUsize::new(1).expect("One should be greater than zero"),
            move |_shard_id| {
                create_raw_xdb_connections(fb, db_address.clone(), read_con_type, readonly).boxify()
            },
        )
    }

    fn with_connection_factory(
        delay: BlobDelay,
        label: String,
        shard_num: NonZeroUsize,
        connection_factory: impl Fn(usize) -> BoxFuture<SqlConnections, Error>,
    ) -> BoxFuture<CountedSqlblob, Error> {
        let shard_count = shard_num.get();

        let futs: Vec<_> = (0..shard_count)
            .into_iter()
            .map(|shard| connection_factory(shard))
            .collect();

        join_all(futs)
            .map(move |shard_connections| {
                let mut write_connections = Vec::with_capacity(shard_count);
                let mut read_connections = Vec::with_capacity(shard_count);
                let mut read_master_connections = Vec::with_capacity(shard_count);

                for connections in shard_connections {
                    write_connections.push(connections.write_connection);
                    read_connections.push(connections.read_connection);
                    read_master_connections.push(connections.read_master_connection);
                }

                let write_connections = Arc::new(write_connections);
                let read_connections = Arc::new(read_connections);
                let read_master_connections = Arc::new(read_master_connections);

                Self::counted(
                    Self {
                        data_store: Arc::new(DataSqlStore::new(
                            shard_num,
                            write_connections.clone(),
                            read_connections.clone(),
                            read_master_connections.clone(),
                            delay.clone(),
                        )),
                        chunk_store: Arc::new(ChunkSqlStore::new(
                            shard_num,
                            write_connections,
                            read_connections,
                            read_master_connections,
                            delay,
                        )),
                    },
                    label,
                )
            })
            .boxify()
    }

    pub fn with_sqlite_in_memory() -> Result<CountedSqlblob> {
        Self::with_sqlite(|_| {
            let con = open_sqlite_in_memory()?;
            con.execute_batch(Self::CREATION_QUERY)?;
            Ok(con)
        })
    }

    pub fn with_sqlite_path<P: Into<PathBuf>>(
        path: P,
        readonly_storage: bool,
    ) -> Result<CountedSqlblob> {
        let pathbuf = path.into();
        Self::with_sqlite(move |shard_id| {
            let con = open_sqlite_path(
                &pathbuf.join(format!("shard_{}.sqlite", shard_id)),
                readonly_storage,
            )?;
            // When opening an sqlite database we might already have the proper tables in it, so ignore
            // errors from table creation
            let _ = con.execute_batch(Self::CREATION_QUERY);
            Ok(con)
        })
    }

    fn with_sqlite<F>(mut constructor: F) -> Result<CountedSqlblob>
    where
        F: FnMut(usize) -> Result<SqliteConnection>,
    {
        let mut cons = Vec::new();

        for i in 0..SQLITE_SHARD_NUM.get() {
            cons.push(Connection::with_sqlite(constructor(i)?));
        }

        let cons = Arc::new(cons);

        Ok(Self::counted(
            Self {
                data_store: Arc::new(DataSqlStore::new(
                    SQLITE_SHARD_NUM,
                    cons.clone(),
                    cons.clone(),
                    cons.clone(),
                    BlobDelay::dummy(SQLITE_SHARD_NUM),
                )),
                chunk_store: Arc::new(ChunkSqlStore::new(
                    SQLITE_SHARD_NUM,
                    cons.clone(),
                    cons.clone(),
                    cons,
                    BlobDelay::dummy(SQLITE_SHARD_NUM),
                )),
            },
            "sqlite".into(),
        ))
    }

    const CREATION_QUERY: &'static str = include_str!("../schema/sqlite-sqlblob.sql");

    fn counted(self, label: String) -> CountedBlobstore<Self> {
        CountedBlobstore::new(format!("{}.{}", COUNTED_ID, label), self)
    }

    #[cfg(test)]
    pub(crate) fn get_data_store(&self) -> &DataSqlStore {
        &self.data_store
    }
}

impl fmt::Debug for Sqlblob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sqlblob").finish()
    }
}

impl Blobstore for Sqlblob {
    fn get(&self, _ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreGetData>, Error> {
        cloned!(self.data_store, self.chunk_store);

        async move {
            let chunked = data_store.get(&key).await?;
            if let Some(chunked) = chunked {
                let fetch_chunks: FuturesOrdered<_> = (0..chunked.count)
                    .map(|chunk_num| {
                        chunk_store.get(&chunked.id, chunk_num, chunked.chunking_method)
                    })
                    .collect();
                let blob: BytesMut = fetch_chunks.try_concat().await?;
                let meta = BlobstoreMetadata::new(Some(chunked.ctime));
                Ok(Some(BlobstoreGetData::new(
                    meta,
                    BlobstoreBytes::from_bytes(blob.freeze()),
                )))
            } else {
                Ok(None)
            }
        }
        .boxed()
        .compat()
        .boxify()
    }

    fn put(&self, _ctx: CoreContext, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        if key.as_bytes().len() > MAX_KEY_SIZE {
            return Err(format_err!(
                "Key {} exceeded max key size {}",
                key,
                MAX_KEY_SIZE
            ))
            .into_future()
            .boxify();
        }

        let chunking_method = ChunkingMethod::ByContentHashBlake2;
        let chunk_key = {
            let mut hash_context = HashContext::new(b"sqlblob");
            hash_context.update(value.as_bytes());
            hash_context.finish().to_hex()
        };

        cloned!(self.data_store, self.chunk_store);
        async move {
            let chunks = value.as_bytes().chunks(CHUNK_SIZE);
            let chunk_count = chunks.len().try_into()?;
            for (chunk_num, value) in chunks.enumerate() {
                chunk_store
                    .put(
                        chunk_key.as_str(),
                        chunk_num.try_into()?,
                        chunking_method,
                        value,
                    )
                    .await?;
            }
            let ctime = {
                match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
                    Ok(offset) => offset.as_secs().try_into(),
                    Err(negative) => negative.duration().as_secs().try_into().map(|v: i64| -v),
                }
            }?;
            data_store
                .put(
                    &key,
                    ctime,
                    chunk_key.as_str(),
                    chunk_count,
                    chunking_method,
                )
                .await
        }
        .boxed()
        .compat()
        .boxify()
    }

    fn is_present(&self, _ctx: CoreContext, key: String) -> BoxFuture<bool, Error> {
        cloned!(self.data_store);
        async move { data_store.is_present(&key).await }
            .boxed()
            .compat()
            .boxify()
    }
}
