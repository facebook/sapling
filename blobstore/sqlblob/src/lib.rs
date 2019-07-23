// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

mod cache;
mod store;

use crate::cache::{ChunkCacheTranslator, DataCacheTranslator, SqlblobCacheOps};
use crate::store::{ChunkSqlStore, DataSqlStore};
use blobstore::{Blobstore, CountedBlobstore};
use cacheblob::{dummy::DummyCache, MemcacheOps};
use cloned::cloned;
use context::CoreContext;
use failure_ext::{format_err, Error, Result};
use futures::future::join_all;
use futures::prelude::*;
use futures_ext::{BoxFuture, FutureExt};
use memcache::MEMCACHE_VALUE_MAX_SIZE;
use mononoke_types::{BlobstoreBytes, RepositoryId};
use sql::{rusqlite::Connection as SqliteConnection, Connection};
use sql_ext::{
    create_myrouter_connections, create_raw_xdb_connections, PoolSizeConfig, SqlConnections,
};
use stats::{define_stats, Timeseries};
use std::fmt;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;

// Leaving some space for metadata
const MAX_KEY_SIZE: usize = 200;
// In order to store blobs that can be stored in Memcache as well use the same max size as memcache
// does, but leave some extra bytes for metadata
const CHUNK_SIZE: usize = MEMCACHE_VALUE_MAX_SIZE - 1000;
const SQLITE_SHARD_NUM: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(100) };

const COUNTED_ID: &str = "sqlblob";
pub type CountedSqlblob = CountedBlobstore<Sqlblob>;

define_stats! {
    prefix = "mononoke.blobstore.sqlblob";
    data_cache_hit_permille: timeseries(AVG, COUNT),
    chunk_cache_hit_permille: timeseries(AVG, COUNT),
}

enum DataEntry {
    Data(BlobstoreBytes),
    InChunk(NonZeroUsize),
}

fn i32_to_non_zero_usize(val: i32) -> Option<NonZeroUsize> {
    if val <= 0 {
        None
    } else {
        NonZeroUsize::new(val as usize)
    }
}

pub struct Sqlblob {
    data_store: DataSqlStore,
    chunk_store: ChunkSqlStore,
    data_cache: SqlblobCacheOps<DataCacheTranslator>,
    chunk_cache: SqlblobCacheOps<ChunkCacheTranslator>,
}

impl Sqlblob {
    pub fn with_myrouter(
        repo_id: RepositoryId,
        shardmap: String,
        port: u16,
        shard_num: NonZeroUsize,
    ) -> BoxFuture<CountedSqlblob, Error> {
        Self::with_connection_factory(repo_id, shardmap.clone(), shard_num, move |shard_id| {
            Ok(create_myrouter_connections(
                shardmap.clone(),
                Some(shard_id),
                port,
                PoolSizeConfig::for_sharded_connection(),
                "blobstore".into(),
            ))
            .into_future()
            .boxify()
        })
    }

    pub fn with_raw_xdb_shardmap(
        repo_id: RepositoryId,
        shardmap: String,
        shard_num: NonZeroUsize,
    ) -> BoxFuture<CountedSqlblob, Error> {
        Self::with_connection_factory(repo_id, shardmap.clone(), shard_num, move |shard_id| {
            create_raw_xdb_connections(format!("{}.{}", shardmap, shard_id)).boxify()
        })
    }

    fn with_connection_factory(
        repo_id: RepositoryId,
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
                struct Cons {
                    write_connection: Vec<Connection>,
                    read_connection: Vec<Connection>,
                    read_master_connection: Vec<Connection>,
                }

                let mut cons = Cons {
                    write_connection: Vec::with_capacity(shard_count),
                    read_connection: Vec::with_capacity(shard_count),
                    read_master_connection: Vec::with_capacity(shard_count),
                };

                for conn in shard_connections {
                    let SqlConnections {
                        write_connection,
                        read_connection,
                        read_master_connection,
                    } = conn;

                    cons.write_connection.push(write_connection);
                    cons.read_connection.push(read_connection);
                    cons.read_master_connection.push(read_master_connection);
                }

                let write_connection = Arc::new(cons.write_connection);
                let read_connection = Arc::new(cons.read_connection);
                let read_master_connection = Arc::new(cons.read_master_connection);

                Self::counted(
                    Self {
                        data_store: DataSqlStore::new(
                            repo_id,
                            shard_num,
                            write_connection.clone(),
                            read_connection.clone(),
                            read_master_connection.clone(),
                        ),
                        chunk_store: ChunkSqlStore::new(
                            repo_id,
                            shard_num,
                            write_connection,
                            read_connection,
                            read_master_connection,
                        ),
                        data_cache: SqlblobCacheOps::new(
                            Arc::new(
                                MemcacheOps::new("sqlblob.data", repo_id.id())
                                    .expect("failed to create MemcacheOps"),
                            ),
                            DataCacheTranslator::new(repo_id),
                        ),
                        chunk_cache: SqlblobCacheOps::new(
                            Arc::new(
                                MemcacheOps::new("sqlblob.chunk", repo_id.id())
                                    .expect("failed to create MemcacheOps"),
                            ),
                            ChunkCacheTranslator::new(repo_id),
                        ),
                    },
                    label,
                )
            })
            .boxify()
    }

    pub fn with_sqlite_in_memory(repo_id: RepositoryId) -> Result<CountedSqlblob> {
        Self::with_sqlite(repo_id, |_| {
            let con = SqliteConnection::open_in_memory()?;
            con.execute_batch(Self::get_up_query())?;
            Ok(con)
        })
    }

    pub fn with_sqlite_path<P: Into<PathBuf>>(
        repo_id: RepositoryId,
        path: P,
    ) -> Result<CountedSqlblob> {
        let path = path.into();
        Self::with_sqlite(repo_id, move |shard_id| {
            let con = SqliteConnection::open(path.join(format!("shard_{}.sqlite", shard_id)))?;
            // When opening an sqlite database we might already have the proper tables in it, so ignore
            // errors from table creation
            let _ = con.execute_batch(Self::get_up_query());
            Ok(con)
        })
    }

    fn with_sqlite<F>(repo_id: RepositoryId, mut constructor: F) -> Result<CountedSqlblob>
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
                data_store: DataSqlStore::new(
                    repo_id,
                    SQLITE_SHARD_NUM,
                    cons.clone(),
                    cons.clone(),
                    cons.clone(),
                ),
                chunk_store: ChunkSqlStore::new(
                    repo_id,
                    SQLITE_SHARD_NUM,
                    cons.clone(),
                    cons.clone(),
                    cons,
                ),
                data_cache: SqlblobCacheOps::new(
                    Arc::new(DummyCache {}),
                    DataCacheTranslator::new(repo_id),
                ),
                chunk_cache: SqlblobCacheOps::new(
                    Arc::new(DummyCache {}),
                    ChunkCacheTranslator::new(repo_id),
                ),
            },
            "sqlite".into(),
        ))
    }

    fn counted(self, label: String) -> CountedSqlblob {
        CountedBlobstore::new(format!("{}.{}", COUNTED_ID, label), self)
    }

    fn get_up_query() -> &'static str {
        include_str!("../schema/sqlite-sqlblob.sql")
    }
}

impl fmt::Debug for Sqlblob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sqlblob").finish()
    }
}

impl Blobstore for Sqlblob {
    fn get(&self, _ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        cloned!(
            self.data_store,
            self.chunk_store,
            self.data_cache,
            self.chunk_cache
        );

        self.data_cache
            .get(&key)
            .and_then({
                cloned!(data_store, data_cache, key);
                move |maybe_value| match maybe_value {
                    Some(value) => {
                        STATS::data_cache_hit_permille.add_value(1000);
                        Ok(Some(value)).into_future().left_future()
                    }
                    None => {
                        STATS::data_cache_hit_permille.add_value(0);
                        data_store
                            .get(&key)
                            .map(move |maybe_entry| {
                                maybe_entry.map(|entry| data_cache.put(&key, entry))
                            })
                            .right_future()
                    }
                }
            })
            .and_then(move |maybe_entry| match maybe_entry {
                None => Ok(None).into_future().left_future(),
                Some(DataEntry::Data(value)) => Ok(Some(value)).into_future().left_future(),
                Some(DataEntry::InChunk(num_of_chunks)) => {
                    let chunk_fut: Vec<_> = (0..num_of_chunks.get() as u32)
                        .map(move |chunk_id| {
                            cloned!(chunk_store, chunk_cache, key);
                            chunk_cache
                                .get(&(key.clone(), chunk_id))
                                .and_then(move |maybe_chunk| match maybe_chunk {
                                    Some(chunk) => {
                                        STATS::chunk_cache_hit_permille.add_value(1000);
                                        Ok(chunk).into_future().left_future()
                                    }
                                    None => {
                                        STATS::chunk_cache_hit_permille.add_value(0);
                                        chunk_store
                                            .get(&key, chunk_id)
                                            .map(move |chunk| {
                                                chunk_cache.put(&(key.clone(), chunk_id), chunk)
                                            })
                                            .right_future()
                                    }
                                })
                        })
                        .collect();

                    join_all(chunk_fut)
                        .map(|chunks| {
                            Some(BlobstoreBytes::from_bytes(
                                chunks
                                    .into_iter()
                                    .map(BlobstoreBytes::into_bytes)
                                    .flatten()
                                    .collect::<Vec<u8>>(),
                            ))
                        })
                        .right_future()
                }
            })
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

        // Store blobs that can be stored in Memcache as well
        if value.len() < CHUNK_SIZE {
            self.data_store.put(&key, &DataEntry::Data(value)).boxify()
        } else {
            cloned!(self.data_store, self.chunk_store);
            data_store
                .is_present(&key)
                .and_then(move |is_present| {
                    if is_present {
                        Ok(()).into_future().left_future()
                    } else {
                        let chunk_fut: Vec<_> = value
                            .as_bytes()
                            .chunks(CHUNK_SIZE)
                            .enumerate()
                            .map(|(chunk_id, chunk)| chunk_store.put(&key, chunk_id as u32, chunk))
                            .collect();

                        join_all(chunk_fut)
                            .and_then(move |chunks| {
                                data_store.put(
                                    &key,
                                    &DataEntry::InChunk(
                                        NonZeroUsize::new(chunks.len())
                                            .expect("No way this is zero"),
                                    ),
                                )
                            })
                            .right_future()
                    }
                })
                .boxify()
        }
    }

    fn is_present(&self, _ctx: CoreContext, key: String) -> BoxFuture<bool, Error> {
        self.data_store.is_present(&key).boxify()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{distributions::Alphanumeric, thread_rng, Rng, RngCore};
    use tokio;

    #[test]
    fn read_write() {
        let ctx = CoreContext::test_mock();
        // Generate unique keys.
        let suffix: String = thread_rng().sample_iter(&Alphanumeric).take(10).collect();
        let key = format!("manifoldblob_test_{}", suffix);

        let bs = Arc::new(Sqlblob::with_sqlite_in_memory(RepositoryId::new(1234)).unwrap());

        let mut bytes_in = [0u8; 64];
        thread_rng().fill_bytes(&mut bytes_in);

        let blobstore_bytes = BlobstoreBytes::from_bytes(&bytes_in as &[u8]);

        let fut = bs.is_present(ctx.clone(), key.clone())
                .map(|is_present| assert!(!is_present, "Blob should not exist yet"))
                // Write a blob.
                .and_then({cloned!(ctx, bs, key); move |()| bs.put(ctx, key, blobstore_bytes)})
                // Read it back and verify it.
                .and_then({cloned!(ctx, bs, key); move |()| bs.get(ctx, key)})
                .map(move |bytes_out| {
                    assert_eq!(&bytes_in.to_vec(), bytes_out.unwrap().as_bytes());
                })
                .and_then({cloned!(ctx); move |()| bs.is_present(ctx, key)})
                .map(|is_present| assert!(is_present, "Blob should exist now"))
                .map_err(|err| panic!("{:#?}", err));

        tokio::run(fut);
    }
}
