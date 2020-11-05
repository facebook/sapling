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
use anyhow::{bail, format_err, Error, Result};
use blobstore::{
    Blobstore, BlobstoreGetData, BlobstoreMetadata, BlobstorePutOps, BlobstoreWithLink,
    CountedBlobstore, OverwriteStatus, PutBehaviour,
};
use bytes::BytesMut;
use cached_config::{ConfigHandle, ConfigStore, TestSource};
use cloned::cloned;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::{
    future::{self, BoxFuture, FutureExt, TryFutureExt},
    stream::{FuturesOrdered, FuturesUnordered, Stream, TryStreamExt},
};
use futures_ext::{try_boxfuture, BoxFuture as BoxFuture01, FutureExt as _};
use futures_old::future::join_all;
use futures_old::prelude::*;
use mononoke_types::{hash::Context as HashContext, BlobstoreBytes};
use sql::{rusqlite::Connection as SqliteConnection, Connection};
use sql_ext::{
    facebook::{
        create_myrouter_connections, create_mysql_pool_sharded, create_mysql_pool_unsharded,
        create_raw_xdb_connections, PoolSizeConfig, ReadConnectionType,
    },
    open_sqlite_in_memory, open_sqlite_path, SqlConnections, SqlShardedConnections,
};
use std::convert::TryInto;
use std::fmt;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use xdb_gc_structs::XdbGc;

// Leaving some space for metadata
const MAX_KEY_SIZE: usize = 200;
// MySQL wants multiple chunks, each around 1 MiB, as a tradeoff between query latency and replication lag
const CHUNK_SIZE: usize = 1024 * 1024;
const SQLITE_SHARD_NUM: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(2) };
const GC_GENERATION_PATH: &str = "scm/mononoke/xdb_gc/default";

// Test setup data
const UPDATE_FREQUENCY: Duration = Duration::from_millis(1);
const INITIAL_VERSION: u64 = 0;

const COUNTED_ID: &str = "sqlblob";
pub type CountedSqlblob = CountedBlobstore<Sqlblob>;

pub struct Sqlblob {
    data_store: Arc<DataSqlStore>,
    chunk_store: Arc<ChunkSqlStore>,
    put_behaviour: PutBehaviour,
}

fn get_gc_config_handle(config_store: &ConfigStore) -> Result<ConfigHandle<XdbGc>> {
    config_store.get_config_handle(GC_GENERATION_PATH.to_string())
}

impl Sqlblob {
    pub fn with_myrouter(
        fb: FacebookInit,
        shardmap: String,
        port: u16,
        read_con_type: ReadConnectionType,
        shard_num: NonZeroUsize,
        readonly: bool,
        put_behaviour: PutBehaviour,
        config_store: &ConfigStore,
    ) -> BoxFuture01<CountedSqlblob, Error> {
        let delay = try_boxfuture!(myadmin_delay::sharded(fb, shardmap.clone(), shard_num));
        Self::with_connection_factory(
            delay,
            shardmap.clone(),
            shard_num,
            put_behaviour,
            move |shard_id| {
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
            },
            config_store,
        )
    }

    pub fn with_myrouter_unsharded(
        fb: FacebookInit,
        db_address: String,
        port: u16,
        read_con_type: ReadConnectionType,
        readonly: bool,
        put_behaviour: PutBehaviour,
        config_store: &ConfigStore,
    ) -> BoxFuture01<CountedSqlblob, Error> {
        let delay = try_boxfuture!(myadmin_delay::single(fb, db_address.clone()));
        Self::with_connection_factory(
            delay,
            db_address.clone(),
            NonZeroUsize::new(1).expect("One should be greater than zero"),
            put_behaviour,
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
            config_store,
        )
    }

    pub fn with_mysql(
        fb: FacebookInit,
        shardmap: String,
        shard_num: NonZeroUsize,
        read_con_type: ReadConnectionType,
        readonly: bool,
        put_behaviour: PutBehaviour,
        config_store: &ConfigStore,
    ) -> BoxFuture01<CountedSqlblob, Error> {
        let delay = try_boxfuture!(myadmin_delay::sharded(fb, shardmap.clone(), shard_num));
        let config_handle = try_boxfuture!(get_gc_config_handle(config_store));

        let shard_num_us = shard_num.clone().get();
        create_mysql_pool_sharded(
            fb,
            shardmap.clone(),
            shard_num_us,
            read_con_type,
            PoolSizeConfig::for_mysql_sharded(),
            readonly,
        )
        .map(
            |
                SqlShardedConnections {
                    read_connections,
                    read_master_connections,
                    write_connections,
                },
            | {
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
                            config_handle,
                        )),
                        put_behaviour,
                    },
                    shardmap,
                )
            },
        )
        .into_future()
        .boxify()
    }

    pub fn with_mysql_unsharded(
        fb: FacebookInit,
        db_address: String,
        read_con_type: ReadConnectionType,
        readonly: bool,
        put_behaviour: PutBehaviour,
        config_store: &ConfigStore,
    ) -> BoxFuture01<CountedSqlblob, Error> {
        let delay = try_boxfuture!(myadmin_delay::single(fb, db_address.clone()));
        Self::with_connection_factory(
            delay,
            db_address.clone(),
            NonZeroUsize::new(1).expect("One should be greater than zero"),
            put_behaviour,
            move |_shard_id| {
                create_mysql_pool_unsharded(
                    fb,
                    db_address.clone(),
                    read_con_type,
                    PoolSizeConfig::for_regular_connection(),
                    readonly,
                )
                .into_future()
                .boxify()
            },
            config_store,
        )
    }

    pub fn with_raw_xdb_shardmap(
        fb: FacebookInit,
        shardmap: String,
        read_con_type: ReadConnectionType,
        shard_num: NonZeroUsize,
        readonly: bool,
        put_behaviour: PutBehaviour,
        config_store: &ConfigStore,
    ) -> BoxFuture01<CountedSqlblob, Error> {
        let delay = try_boxfuture!(myadmin_delay::sharded(fb, shardmap.clone(), shard_num));
        Self::with_connection_factory(
            delay,
            shardmap.clone(),
            shard_num,
            put_behaviour,
            move |shard_id| {
                create_raw_xdb_connections(
                    fb,
                    format!("{}.{}", shardmap, shard_id),
                    read_con_type,
                    readonly,
                )
                .boxify()
            },
            config_store,
        )
    }

    pub fn with_raw_xdb_unsharded(
        fb: FacebookInit,
        db_address: String,
        read_con_type: ReadConnectionType,
        readonly: bool,
        put_behaviour: PutBehaviour,
        config_store: &ConfigStore,
    ) -> BoxFuture01<CountedSqlblob, Error> {
        let delay = try_boxfuture!(myadmin_delay::single(fb, db_address.clone()));
        Self::with_connection_factory(
            delay,
            db_address.clone(),
            NonZeroUsize::new(1).expect("One should be greater than zero"),
            put_behaviour,
            move |_shard_id| {
                create_raw_xdb_connections(fb, db_address.clone(), read_con_type, readonly).boxify()
            },
            config_store,
        )
    }

    fn with_connection_factory(
        delay: BlobDelay,
        label: String,
        shard_num: NonZeroUsize,
        put_behaviour: PutBehaviour,
        connection_factory: impl Fn(usize) -> BoxFuture01<SqlConnections, Error>,
        config_store: &ConfigStore,
    ) -> BoxFuture01<CountedSqlblob, Error> {
        let shard_count = shard_num.get();

        let config_handle = try_boxfuture!(get_gc_config_handle(config_store));

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
                            config_handle,
                        )),
                        put_behaviour,
                    },
                    label,
                )
            })
            .boxify()
    }

    pub fn with_sqlite_in_memory(
        put_behaviour: PutBehaviour,
        config_store: &ConfigStore,
    ) -> Result<CountedSqlblob> {
        Self::with_sqlite(
            put_behaviour,
            |_| {
                let con = open_sqlite_in_memory()?;
                con.execute_batch(Self::CREATION_QUERY)?;
                Ok(con)
            },
            config_store,
        )
    }

    pub fn with_sqlite_path<P: Into<PathBuf>>(
        path: P,
        readonly_storage: bool,
        put_behaviour: PutBehaviour,
        config_store: &ConfigStore,
    ) -> Result<CountedSqlblob> {
        let pathbuf = path.into();
        Self::with_sqlite(
            put_behaviour,
            move |shard_id| {
                let con = open_sqlite_path(
                    &pathbuf.join(format!("shard_{}.sqlite", shard_id)),
                    readonly_storage,
                )?;
                // When opening an sqlite database we might already have the proper tables in it, so ignore
                // errors from table creation
                let _ = con.execute_batch(Self::CREATION_QUERY);
                Ok(con)
            },
            config_store,
        )
    }

    fn with_sqlite<F>(
        put_behaviour: PutBehaviour,
        mut constructor: F,
        config_store: &ConfigStore,
    ) -> Result<CountedSqlblob>
    where
        F: FnMut(usize) -> Result<SqliteConnection>,
    {
        let mut cons = Vec::new();

        for i in 0..SQLITE_SHARD_NUM.get() {
            cons.push(Connection::with_sqlite(constructor(i)?));
        }

        let cons = Arc::new(cons);

        // SQLite is predominately intended for tests, and has less concurrency
        // issues relating to GC, so cope with missing configerator
        let config_handle = get_gc_config_handle(config_store)
            .or_else(|_| get_gc_config_handle(&(get_test_config_store().1)))?;

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
                    config_handle,
                )),
                put_behaviour,
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

    pub fn get_keys_from_shard(&self, shard_num: usize) -> impl Stream<Item = Result<String>> {
        self.data_store.get_keys_from_shard(shard_num)
    }

    pub async fn get_chunk_generations(&self, key: &str) -> Result<Vec<Option<u64>>> {
        let chunked = self.data_store.get(key).await?;
        if let Some(chunked) = chunked {
            let fetch_chunk_generations: FuturesOrdered<_> = (0..chunked.count)
                .map(|chunk_num| {
                    self.chunk_store
                        .get_generation(&chunked.id, chunk_num, chunked.chunking_method)
                })
                .collect();
            fetch_chunk_generations.try_collect().await
        } else {
            bail!("key does not exist");
        }
    }

    pub async fn set_generation(&self, key: &str) -> Result<(), Error> {
        let chunked = self.data_store.get(key).await?;
        if let Some(chunked) = chunked {
            let set_chunk_generations: FuturesUnordered<_> = (0..chunked.count)
                .map(|chunk_num| {
                    self.chunk_store
                        .set_generation(&chunked.id, chunk_num, chunked.chunking_method)
                })
                .collect();
            set_chunk_generations.try_collect().await
        } else {
            bail!("key does not exist");
        }
    }
}

impl fmt::Debug for Sqlblob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sqlblob").finish()
    }
}

impl Blobstore for Sqlblob {
    fn get(
        &self,
        _ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<Option<BlobstoreGetData>, Error>> {
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
    }

    fn is_present(
        &self,
        _ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<bool, Error>> {
        cloned!(self.data_store);
        async move { data_store.is_present(&key).await }.boxed()
    }

    fn put(
        &self,
        ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'static, Result<(), Error>> {
        BlobstorePutOps::put_with_status(self, ctx, key, value)
            .map_ok(|_| ())
            .boxed()
    }
}

impl BlobstorePutOps for Sqlblob {
    fn put_explicit(
        &self,
        ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: PutBehaviour,
    ) -> BoxFuture<'static, Result<OverwriteStatus, Error>> {
        if key.as_bytes().len() > MAX_KEY_SIZE {
            return future::err(format_err!(
                "Key {} exceeded max key size {}",
                key,
                MAX_KEY_SIZE
            ))
            .boxed();
        }

        let chunking_method = ChunkingMethod::ByContentHashBlake2;
        let chunk_key = {
            let mut hash_context = HashContext::new(b"sqlblob");
            hash_context.update(value.as_bytes());
            hash_context.finish().to_hex()
        };

        cloned!(self.data_store, self.chunk_store);
        let put_fut = {
            cloned!(key);
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
                let res = data_store
                    .put(
                        &key,
                        ctime,
                        chunk_key.as_str(),
                        chunk_count,
                        chunking_method,
                    )
                    .await;
                res.map(|()| OverwriteStatus::NotChecked)
            }
            .boxed()
        };

        match put_behaviour {
            PutBehaviour::Overwrite => put_fut,
            PutBehaviour::IfAbsent | PutBehaviour::OverwriteAndLog => {
                let exists = self.is_present(ctx, key);
                async move {
                    if exists.await? {
                        if put_behaviour.should_overwrite() {
                            put_fut.await?;
                            Ok(OverwriteStatus::Overwrote)
                        } else {
                            // discard the put
                            let _ = put_fut;
                            Ok(OverwriteStatus::Prevented)
                        }
                    } else {
                        put_fut.await?;
                        Ok(OverwriteStatus::New)
                    }
                }
                .boxed()
            }
        }
    }

    fn put_with_status(
        &self,
        ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'static, Result<OverwriteStatus, Error>> {
        self.put_explicit(ctx, key, value, self.put_behaviour)
    }
}

impl BlobstoreWithLink for Sqlblob {
    fn link(
        &self,
        _ctx: CoreContext,
        existing_key: String,
        link_key: String,
    ) -> BoxFuture<'static, Result<(), Error>> {
        cloned!(self.data_store);
        async move {
            let existing_data = data_store.get(&existing_key).await?.ok_or_else(|| {
                format_err!("Key {} does not exist in the blobstore", existing_key)
            })?;
            data_store
                .put(
                    &link_key,
                    existing_data.ctime,
                    &existing_data.id,
                    existing_data.count,
                    existing_data.chunking_method,
                )
                .await
        }
        .boxed()
    }
}

pub fn set_test_generations(
    source: &TestSource,
    put_generation: i64,
    mark_generation: i64,
    delete_generation: i64,
    mod_time: u64,
) {
    source.insert_config(
        GC_GENERATION_PATH,
        &serde_json::to_string(&XdbGc {
            put_generation,
            mark_generation,
            delete_generation,
        })
        .expect("Invalid input config somehow"),
        mod_time,
    );
    source.insert_to_refresh(GC_GENERATION_PATH.to_string());
}

pub fn get_test_config_store() -> (Arc<TestSource>, ConfigStore) {
    let test_source = Arc::new(TestSource::new());
    set_test_generations(test_source.as_ref(), 2, 1, 0, INITIAL_VERSION);
    (
        test_source.clone(),
        ConfigStore::new(test_source, UPDATE_FREQUENCY, None),
    )
}
