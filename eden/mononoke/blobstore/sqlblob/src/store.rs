/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::hash::Hasher;
use std::num::NonZeroUsize;
use std::sync::Arc;

use anyhow::{format_err, Error};
use bytes::BytesMut;
use futures::compat::Future01CompatExt;
use sql::{queries, Connection};
use twox_hash::XxHash32;

use crate::delay::BlobDelay;

mod types {
    use sql::mysql_async::{
        prelude::{ConvIr, FromValue},
        FromValueError, Value,
    };

    type FromValueResult<T> = Result<T, FromValueError>;

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum ChunkingMethod {
        ByContentHashBlake2,
    }

    impl From<ChunkingMethod> for Value {
        fn from(dtype: ChunkingMethod) -> Self {
            match dtype {
                ChunkingMethod::ByContentHashBlake2 => Value::Int(1),
            }
        }
    }

    impl ConvIr<ChunkingMethod> for ChunkingMethod {
        fn new(v: Value) -> FromValueResult<Self> {
            match v {
                Value::Int(1) => Ok(ChunkingMethod::ByContentHashBlake2),
                Value::Bytes(ref b) if b == b"1" => Ok(ChunkingMethod::ByContentHashBlake2),
                v => Err(FromValueError(v)),
            }
        }

        fn commit(self) -> ChunkingMethod {
            self
        }

        fn rollback(self) -> Value {
            self.into()
        }
    }

    impl FromValue for ChunkingMethod {
        type Intermediate = ChunkingMethod;
    }
}

pub use self::types::ChunkingMethod;

queries! {
    write InsertData(values: (id: &str, ctime: i64, chunk_id: &str, chunk_count: u32, chunking_method: ChunkingMethod)) {
        insert_or_ignore,
        "{insert_or_ignore} INTO data (
            id
            , creation_time
            , chunk_id
            , chunk_count
            , chunking_method
        ) VALUES {values}"
    }

    write InsertChunk(values: (id: &str, chunk_num: u32, value: &[u8])) {
        insert_or_ignore,
        "{insert_or_ignore} INTO chunk (
            id
            , chunk_num
            , value
        ) VALUES {values}"
    }

    read SelectData(id: &str) -> (i64, Vec<u8>, u32, ChunkingMethod) {
        "SELECT creation_time, chunk_id, chunk_count, chunking_method
         FROM data
         WHERE id = {id}"
    }

    read SelectIsDataPresent(id: &str) -> (i32) {
        "SELECT 1
         FROM data
         WHERE id = {id}"
    }

    read SelectChunk(id: &str, chunk_num: u32) -> (Vec<u8>) {
        "SELECT value
         FROM chunk
         WHERE id = {id}
           AND chunk_num = {chunk_num}"
    }
}

pub struct Chunked {
    pub id: String,
    pub count: u32,
    pub ctime: i64,
    pub chunking_method: ChunkingMethod,
}

#[derive(Clone)]
pub(crate) struct DataSqlStore {
    shard_count: NonZeroUsize,
    write_connection: Arc<Vec<Connection>>,
    read_connection: Arc<Vec<Connection>>,
    read_master_connection: Arc<Vec<Connection>>,
    delay: BlobDelay,
}

impl DataSqlStore {
    pub(crate) fn new(
        shard_count: NonZeroUsize,
        write_connection: Arc<Vec<Connection>>,
        read_connection: Arc<Vec<Connection>>,
        read_master_connection: Arc<Vec<Connection>>,
        delay: BlobDelay,
    ) -> Self {
        Self {
            shard_count,
            write_connection,
            read_connection,
            read_master_connection,
            delay,
        }
    }

    pub(crate) async fn get(&self, key: &str) -> Result<Option<Chunked>, Error> {
        let shard_id = self.shard(key);

        let rows = {
            let rows = SelectData::query(&self.read_connection[shard_id], &key)
                .compat()
                .await?;
            if rows.is_empty() {
                SelectData::query(&self.read_master_connection[shard_id], &key)
                    .compat()
                    .await?
            } else {
                rows
            }
        };

        Ok(rows
            .into_iter()
            .next()
            .map(|(ctime, chunk_id, chunk_count, chunking_method)| Chunked {
                id: String::from_utf8_lossy(&chunk_id).to_string(),
                count: chunk_count,
                ctime,
                chunking_method,
            }))
    }

    pub(crate) async fn put(
        &self,
        key: &str,
        ctime: i64,
        chunk_id: &str,
        chunk_count: u32,
        chunking_method: ChunkingMethod,
    ) -> Result<(), Error> {
        let shard_id = self.shard(key);

        self.delay.delay(shard_id).await;

        InsertData::query(
            &self.write_connection[shard_id],
            &[(&key, &ctime, &chunk_id, &chunk_count, &chunking_method)],
        )
        .compat()
        .await?;
        Ok(())
    }

    pub(crate) async fn is_present(&self, key: &str) -> Result<bool, Error> {
        let shard_id = self.shard(key);

        let rows = {
            let rows = SelectIsDataPresent::query(&self.read_connection[shard_id], &key)
                .compat()
                .await?;
            if rows.is_empty() {
                SelectIsDataPresent::query(&self.read_master_connection[shard_id], &key)
                    .compat()
                    .await?
            } else {
                rows
            }
        };
        Ok(!rows.is_empty())
    }

    fn shard(&self, key: &str) -> usize {
        let mut hasher = XxHash32::with_seed(0);
        hasher.write(key.as_bytes());
        (hasher.finish() % self.shard_count.get() as u64) as usize
    }
}

#[derive(Clone)]
pub(crate) struct ChunkSqlStore {
    shard_count: NonZeroUsize,
    write_connection: Arc<Vec<Connection>>,
    read_connection: Arc<Vec<Connection>>,
    read_master_connection: Arc<Vec<Connection>>,
    delay: BlobDelay,
}

impl ChunkSqlStore {
    pub(crate) fn new(
        shard_count: NonZeroUsize,
        write_connection: Arc<Vec<Connection>>,
        read_connection: Arc<Vec<Connection>>,
        read_master_connection: Arc<Vec<Connection>>,
        delay: BlobDelay,
    ) -> Self {
        Self {
            shard_count,
            write_connection,
            read_connection,
            read_master_connection,
            delay,
        }
    }

    pub(crate) async fn get(
        &self,
        id: &str,
        chunk_num: u32,
        chunking_method: ChunkingMethod,
    ) -> Result<BytesMut, Error> {
        let shard_id = self.shard(id, chunk_num, chunking_method);

        let rows = {
            let rows = SelectChunk::query(&self.read_connection[shard_id], &id, &chunk_num)
                .compat()
                .await?;
            if rows.is_empty() {
                SelectChunk::query(&self.read_master_connection[shard_id], &id, &chunk_num)
                    .compat()
                    .await?
            } else {
                rows
            }
        };
        rows.into_iter()
            .next()
            .map(|(value,)| (&*value).into())
            .ok_or_else(|| format_err!("Missing chunk with id {} shard {}", chunk_num, shard_id))
    }

    pub(crate) async fn put(
        &self,
        key: &str,
        chunk_num: u32,
        chunking_method: ChunkingMethod,
        value: &[u8],
    ) -> Result<(), Error> {
        let shard_id = self.shard(key, chunk_num, chunking_method);

        self.delay.delay(shard_id).await;
        InsertChunk::query(
            &self.write_connection[shard_id],
            &[(&key, &chunk_num, &value)],
        )
        .compat()
        .await?;
        Ok(())
    }

    fn shard(&self, key: &str, chunk_id: u32, _chunking_method: ChunkingMethod) -> usize {
        let mut hasher = XxHash32::with_seed(0);
        hasher.write(key.as_bytes());
        hasher.write_u32(chunk_id);
        (hasher.finish() % self.shard_count.get() as u64) as usize
    }
}
