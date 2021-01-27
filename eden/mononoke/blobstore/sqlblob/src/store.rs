/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{collections::HashMap, hash::Hasher, num::NonZeroUsize, sync::Arc};

use anyhow::{format_err, Error};
use bytes::BytesMut;
use cached_config::ConfigHandle;
use futures::{
    compat::Future01CompatExt,
    future::TryFutureExt,
    stream::{self, Stream},
};
use sql::{queries, Connection};
use twox_hash::XxHash32;
use xdb_gc_structs::XdbGc;

use crate::delay::BlobDelay;

mod types {
    use sql::mysql;
    use sql::mysql_async::{
        prelude::{ConvIr, FromValue},
        FromValueError, Value,
    };

    type FromValueResult<T> = Result<T, FromValueError>;

    #[derive(Clone, Copy, Debug, PartialEq, mysql::OptTryFromRowField)]
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

    write UpdateData(id: &str, ctime: i64, chunk_id: &str, chunk_count: u32, chunking_method: ChunkingMethod) {
        none,
        "UPDATE data SET
            creation_time = {ctime}
            , chunk_id = {chunk_id}
            , chunk_count = {chunk_count}
            , chunking_method = {chunking_method}
        WHERE id = {id}"
    }

    write InsertChunk(values: (id: &str, chunk_num: u32, value: &[u8])) {
        insert_or_ignore,
        "{insert_or_ignore} INTO chunk (
            id
            , chunk_num
            , value
        ) VALUES {values}"
    }

    write UpdateGeneration(id: &str, generation: u64) {
        none,
        "UPDATE chunk_generation
            SET last_seen_generation = {generation}
            WHERE id = {id} AND last_seen_generation < {generation}"
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

    read GetChunkGeneration(id: &str) -> (u64) {
        "SELECT last_seen_generation
        FROM chunk_generation
        WHERE id = {id}"
    }

    write InsertGeneration(values: (id: &str, generation: u64)) {
        insert_or_ignore,
        "{insert_or_ignore} INTO chunk_generation VALUES {values}"
    }

    read GetAllKeys() -> (Vec<u8>) {
        "SELECT id FROM data"
    }

    read GetGenerationSizes() -> (Option<u64>, u64) {
        "SELECT chunk_generation.last_seen_generation, CAST(SUM(LENGTH(chunk.value)) AS UNSIGNED)
        FROM chunk LEFT JOIN chunk_generation ON chunk.id = chunk_generation.id
        GROUP BY chunk_generation.last_seen_generation"
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

        let res = InsertData::query(
            &self.write_connection[shard_id],
            &[(&key, &ctime, &chunk_id, &chunk_count, &chunking_method)],
        )
        .compat()
        .await?;
        if res.affected_rows() == 0 {
            UpdateData::query(
                &self.write_connection[shard_id],
                &key,
                &ctime,
                &chunk_id,
                &chunk_count,
                &chunking_method,
            )
            .compat()
            .await?;
        }
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

    pub(crate) fn get_keys_from_shard(
        &self,
        shard_num: usize,
    ) -> impl Stream<Item = Result<String, Error>> {
        GetAllKeys::query(&self.read_master_connection[shard_num])
            .compat()
            .map_ok(|keys| {
                stream::iter(
                    keys.into_iter()
                        .map(|(id,)| Ok(String::from_utf8_lossy(&id).to_string())),
                )
            })
            .try_flatten_stream()
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
    gc_generations: ConfigHandle<XdbGc>,
}

impl ChunkSqlStore {
    pub(crate) fn new(
        shard_count: NonZeroUsize,
        write_connection: Arc<Vec<Connection>>,
        read_connection: Arc<Vec<Connection>>,
        read_master_connection: Arc<Vec<Connection>>,
        delay: BlobDelay,
        gc_generations: ConfigHandle<XdbGc>,
    ) -> Self {
        Self {
            shard_count,
            write_connection,
            read_connection,
            read_master_connection,
            delay,
            gc_generations,
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
        UpdateGeneration::query(
            &self.write_connection[shard_id],
            &key,
            &(self.gc_generations.get().put_generation as u64),
        )
        .compat()
        .await?;
        InsertChunk::query(
            &self.write_connection[shard_id],
            &[(&key, &chunk_num, &value)],
        )
        .compat()
        .await?;
        Ok(())
    }

    pub(crate) async fn get_generation(
        &self,
        key: &str,
        chunk_num: u32,
        chunking_method: ChunkingMethod,
    ) -> Result<Option<u64>, Error> {
        let shard_id = self.shard(key, chunk_num, chunking_method);
        let rows = {
            let rows = GetChunkGeneration::query(&self.read_connection[shard_id], &key)
                .compat()
                .await?;
            if rows.is_empty() {
                GetChunkGeneration::query(&self.read_master_connection[shard_id], &key)
                    .compat()
                    .await?
            } else {
                rows
            }
        };
        Ok(rows.into_iter().next().map(|(v,)| v))
    }

    pub(crate) async fn set_generation(
        &self,
        key: &str,
        chunk_num: u32,
        chunking_method: ChunkingMethod,
    ) -> Result<(), Error> {
        let shard_id = self.shard(key, chunk_num, chunking_method);
        // First set the generation if unset, so that future writers will update it.
        // TODO: replace 2 with the fetched generation number
        InsertGeneration::query(
            &self.write_connection[shard_id],
            &[(&key, &(self.gc_generations.get().put_generation as u64))],
        )
        .compat()
        .await?;
        // Then update it in case it already existed
        // TODO: replace 1 with the fetched generation number
        UpdateGeneration::query(
            &self.write_connection[shard_id],
            &key,
            &(self.gc_generations.get().mark_generation as u64),
        )
        .compat()
        .await?;
        Ok(())
    }

    pub(crate) async fn get_chunk_sizes_by_generation(
        &self,
        shard_num: usize,
    ) -> Result<HashMap<Option<u64>, u64>, Error> {
        GetGenerationSizes::query(&self.read_master_connection[shard_num])
            .compat()
            .await
            .map(|s| s.into_iter().collect::<HashMap<_, _>>())
    }

    fn shard(&self, key: &str, chunk_id: u32, _chunking_method: ChunkingMethod) -> usize {
        let mut hasher = XxHash32::with_seed(0);
        hasher.write(key.as_bytes());
        hasher.write_u32(chunk_id);
        (hasher.finish() % self.shard_count.get() as u64) as usize
    }
}
