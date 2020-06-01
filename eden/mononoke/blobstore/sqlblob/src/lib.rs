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

use crate::delay::BlobDelay;
#[cfg(fbcode_build)]
use crate::facebook::myadmin_delay;
#[cfg(not(fbcode_build))]
use crate::myadmin_delay_dummy as myadmin_delay;
use crate::store::{ChunkSqlStore, DataSqlStore};
use anyhow::{format_err, Error, Result};
use blobstore::{Blobstore, BlobstoreGetData, CountedBlobstore};
use cloned::cloned;
use context::CoreContext;
use fbinit::FacebookInit;
use futures_ext::{try_boxfuture, BoxFuture, FutureExt as _};
use futures_old::future::{self, join_all};
use futures_old::prelude::*;
use memcache::MEMCACHE_VALUE_MAX_SIZE;
use mononoke_types::BlobstoreBytes;
use sql::{rusqlite::Connection as SqliteConnection, Connection};
use sql_ext::{
    facebook::{
        create_myrouter_connections, create_raw_xdb_connections, PoolSizeConfig, ReadConnectionType,
    },
    open_sqlite_in_memory, open_sqlite_path, SqlConnections,
};
use std::fmt;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;

// Leaving some space for metadata
const MAX_KEY_SIZE: usize = 200;
// In order to store blobs that can be stored in Memcache as well use the same max size as memcache
// does, but leave some extra bytes for metadata
const CHUNK_SIZE: usize = MEMCACHE_VALUE_MAX_SIZE - 1000;
const SQLITE_SHARD_NUM: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(2) };

const COUNTED_ID: &str = "sqlblob";
pub type CountedSqlblob = CountedBlobstore<Sqlblob>;

enum DataEntry {
    Data(BlobstoreGetData),
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
    delay: BlobDelay,
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
        let delay = try_boxfuture!(myadmin_delay::sharded(fb, &shardmap));
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
        let delay = try_boxfuture!(myadmin_delay::sharded(fb, &shardmap));
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
                        data_store: DataSqlStore::new(
                            shard_num,
                            write_connections.clone(),
                            read_connections.clone(),
                            read_master_connections.clone(),
                        ),
                        chunk_store: ChunkSqlStore::new(
                            shard_num,
                            write_connections,
                            read_connections,
                            read_master_connections,
                        ),
                        delay,
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
                data_store: DataSqlStore::new(
                    SQLITE_SHARD_NUM,
                    cons.clone(),
                    cons.clone(),
                    cons.clone(),
                ),
                chunk_store: ChunkSqlStore::new(SQLITE_SHARD_NUM, cons.clone(), cons.clone(), cons),
                delay: BlobDelay::dummy(),
            },
            "sqlite".into(),
        ))
    }

    const CREATION_QUERY: &'static str = include_str!("../schema/sqlite-sqlblob.sql");

    fn counted(self, label: String) -> CountedBlobstore<Self> {
        CountedBlobstore::new(format!("{}.{}", COUNTED_ID, label), self)
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

        data_store
            .get(&key)
            .and_then(move |maybe_entry| match maybe_entry {
                None => Ok(None).into_future().left_future(),
                Some(DataEntry::Data(value)) => Ok(Some(value)).into_future().left_future(),
                Some(DataEntry::InChunk(num_of_chunks)) => {
                    let chunk_fut: Vec<_> = (0..num_of_chunks.get() as u32)
                        .map(move |chunk_id| chunk_store.get(&key, chunk_id))
                        .collect();

                    join_all(chunk_fut)
                        .map(|chunks| {
                            Some(BlobstoreGetData::from_bytes(
                                chunks
                                    .into_iter()
                                    .map(BlobstoreGetData::into_raw_bytes)
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

        let delay = self.delay.clone();
        if value.len() < CHUNK_SIZE {
            let init_delay = delay.delay();
            let put = self.data_store.put(&key, &DataEntry::Data(value.into()));
            init_delay.and_then(move |_| put).boxify()
        } else {
            cloned!(self.data_store, self.chunk_store, self.delay);
            data_store
                .is_present(&key)
                .and_then(move |is_present| {
                    if is_present {
                        Ok(()).into_future().left_future()
                    } else {
                        let chunk_futs: Vec<_> = value
                            .as_bytes()
                            .chunks(CHUNK_SIZE)
                            .enumerate()
                            .map({
                                |(chunk_id, chunk)| chunk_store.put(&key, chunk_id as u32, chunk)
                            })
                            .collect();
                        let len = chunk_futs.len();

                        let fut = chunk_futs.into_iter().fold(future::ok(()).boxify(), {
                            let delay = delay.clone();
                            move |chain, next| {
                                let delay = delay.clone();
                                chain
                                    .and_then(move |_| delay.delay().and_then(|()| next))
                                    .boxify()
                            }
                        });
                        fut.and_then(move |_| delay.delay())
                            .and_then(move |_| {
                                data_store.put(
                                    &key,
                                    &DataEntry::InChunk(
                                        NonZeroUsize::new(len).expect("No way this is zero"),
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
    use bytes::Bytes;
    use fbinit::FacebookInit;
    use futures::compat::Future01CompatExt;
    use rand::{distributions::Alphanumeric, thread_rng, Rng, RngCore};

    #[fbinit::compat_test]
    async fn read_write(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        // Generate unique keys.
        let suffix: String = thread_rng().sample_iter(&Alphanumeric).take(10).collect();
        let key = format!("manifoldblob_test_{}", suffix);

        let bs = Arc::new(Sqlblob::with_sqlite_in_memory().unwrap());

        let mut bytes_in = [0u8; 64];
        thread_rng().fill_bytes(&mut bytes_in);

        let blobstore_bytes = BlobstoreBytes::from_bytes(Bytes::copy_from_slice(&bytes_in));

        let fut = bs.is_present(ctx.clone(), key.clone())
                .map(|is_present| assert!(!is_present, "Blob should not exist yet"))
                // Write a blob.
                .and_then({cloned!(ctx, bs, key); move |()| bs.put(ctx, key, blobstore_bytes)})
                // Read it back and verify it.
                .and_then({cloned!(ctx, bs, key); move |()| bs.get(ctx, key)})
                .map(move |bytes_out| {
                    assert_eq!(&bytes_in.to_vec(), bytes_out.unwrap().as_raw_bytes());
                })
                .and_then({cloned!(ctx); move |()| bs.is_present(ctx, key)})
                .map(|is_present| assert!(is_present, "Blob should exist now"))
                .map_err(|err| panic!("{:#?}", err));

        fut.compat().await.unwrap()
    }
}
