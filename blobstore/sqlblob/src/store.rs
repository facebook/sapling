// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::hash::Hasher;
use std::num::NonZeroUsize;
use std::sync::Arc;

use failure_ext::{err_msg, format_err, Error};
use futures::prelude::*;
use futures_ext::FutureExt;
use rust_thrift::compact_protocol;
use sql::{queries, Connection};
use twox_hash::XxHash32;

use mononoke_types::BlobstoreBytes;
use sqlblob_thrift::InChunk;

use crate::{i32_to_non_zero_usize, DataEntry};

mod types {
    use sql::mysql_async::{
        prelude::{ConvIr, FromValue},
        FromValueError, Value,
    };

    type FromValueResult<T> = Result<T, FromValueError>;

    #[derive(Clone)]
    pub enum DataType {
        Data,
        InChunk,
    }

    impl From<DataType> for Value {
        fn from(dtype: DataType) -> Self {
            match dtype {
                DataType::Data => Value::Int(1),
                DataType::InChunk => Value::Int(2),
            }
        }
    }

    impl ConvIr<DataType> for DataType {
        fn new(v: Value) -> FromValueResult<Self> {
            match v {
                Value::Int(1) => Ok(DataType::Data),
                Value::Bytes(ref b) if b == b"1" => Ok(DataType::Data),
                Value::Int(2) => Ok(DataType::InChunk),
                Value::Bytes(ref b) if b == b"2" => Ok(DataType::InChunk),
                v => Err(FromValueError(v)),
            }
        }

        fn commit(self) -> DataType {
            self
        }

        fn rollback(self) -> Value {
            self.into()
        }
    }

    impl FromValue for DataType {
        type Intermediate = DataType;
    }
}

use self::types::DataType;

queries! {
    write InsertData(values: (id: &str, dtype: DataType, value: &[u8])) {
        insert_or_ignore,
        "{insert_or_ignore} INTO data (
            id
            , type
            , value
        ) VALUES {values}"
    }

    write InsertChunk(values: (id: &str, chunk_id: u32, value: &[u8])) {
        insert_or_ignore,
        "{insert_or_ignore} INTO chunk (
            id
            , chunk_id
            , value
        ) VALUES {values}"
    }

    read SelectData(id: String) -> (DataType, Vec<u8>) {
        "SELECT type, value
         FROM data
         WHERE id = {id}"
    }

    read SelectIsDataPresent(id: String) -> (i32) {
        "SELECT 1
         FROM data
         WHERE id = {id}"
    }

    read SelectChunk(id: String, chunk_id: u32) -> (Vec<u8>) {
        "SELECT value
         FROM chunk
         WHERE id = {id}
           AND chunk_id = {chunk_id}"
    }
}

#[derive(Clone)]
pub(crate) struct DataSqlStore {
    shard_num: NonZeroUsize,
    write_connection: Arc<Vec<Connection>>,
    read_connection: Arc<Vec<Connection>>,
    read_master_connection: Arc<Vec<Connection>>,
}

impl DataSqlStore {
    pub(crate) fn new(
        shard_num: NonZeroUsize,
        write_connection: Arc<Vec<Connection>>,
        read_connection: Arc<Vec<Connection>>,
        read_master_connection: Arc<Vec<Connection>>,
    ) -> Self {
        Self {
            shard_num,
            write_connection,
            read_connection,
            read_master_connection,
        }
    }

    pub(crate) fn get(&self, key: &str) -> impl Future<Item = Option<DataEntry>, Error = Error> {
        let key = key.to_owned();
        let shard_id = self.shard(&key);
        let read_master_connection = self.read_master_connection[shard_id - 1].clone();

        SelectData::query(&self.read_connection[shard_id - 1], &key)
            .and_then(move |rows| match rows.into_iter().next() {
                Some(row) => Ok(Some(row)).into_future().left_future(),
                None => SelectData::query(&read_master_connection, &key)
                    .map(|rows| rows.into_iter().next())
                    .right_future(),
            })
            .and_then(move |rows| match rows.into_iter().next() {
                None => Ok(None),
                Some((DataType::Data, value)) => {
                    Ok(Some(DataEntry::Data(BlobstoreBytes::from_bytes(value))))
                }
                Some((DataType::InChunk, value)) => match compact_protocol::deserialize(value) {
                    Ok(InChunk::num_of_chunks(num_of_chunks)) => {
                        match i32_to_non_zero_usize(num_of_chunks) {
                            None => Err(err_msg("Encoded number of chunks was invalid")),
                            Some(num_of_chunks) => Ok(Some(DataEntry::InChunk(num_of_chunks))),
                        }
                    }
                    Err(_) | Ok(InChunk::UnknownField(_)) => {
                        Err(err_msg("Failed to deserialize InChunk data"))
                    }
                },
            })
    }

    pub(crate) fn put(
        &self,
        key: &str,
        entry: &DataEntry,
    ) -> impl Future<Item = (), Error = Error> {
        let shard_id = self.shard(key);

        let (dtype, value) = match entry {
            DataEntry::Data(ref value) => (DataType::Data, value.clone()),
            DataEntry::InChunk(num_of_chunks) => {
                let in_chunk_meta = InChunk::num_of_chunks(num_of_chunks.get() as i32);
                let in_chunk_meta = compact_protocol::serialize(&in_chunk_meta);
                (DataType::InChunk, BlobstoreBytes::from_bytes(in_chunk_meta))
            }
        };

        InsertData::query(
            &self.write_connection[shard_id - 1],
            &[(&key, &dtype, &value.into_bytes().as_ref())],
        )
        .map(|_| ())
    }

    pub(crate) fn is_present(&self, key: &str) -> impl Future<Item = bool, Error = Error> {
        let key = key.to_owned();
        let shard_id = self.shard(&key);
        let read_master_connection = self.read_master_connection[shard_id - 1].clone();

        SelectIsDataPresent::query(&self.read_connection[shard_id - 1], &key).and_then(
            move |rows| {
                if rows.into_iter().next().is_some() {
                    Ok(true).into_future().left_future()
                } else {
                    SelectIsDataPresent::query(&read_master_connection, &key)
                        .map(|rows| rows.into_iter().next().is_some())
                        .right_future()
                }
            },
        )
    }

    fn shard(&self, key: &str) -> usize {
        let mut hasher = XxHash32::with_seed(0);
        hasher.write(key.as_bytes());
        ((hasher.finish() % self.shard_num.get() as u64) + 1) as usize
    }
}

#[derive(Clone)]
pub(crate) struct ChunkSqlStore {
    shard_num: NonZeroUsize,
    write_connection: Arc<Vec<Connection>>,
    read_connection: Arc<Vec<Connection>>,
    read_master_connection: Arc<Vec<Connection>>,
}

impl ChunkSqlStore {
    pub(crate) fn new(
        shard_num: NonZeroUsize,
        write_connection: Arc<Vec<Connection>>,
        read_connection: Arc<Vec<Connection>>,
        read_master_connection: Arc<Vec<Connection>>,
    ) -> Self {
        Self {
            shard_num,
            write_connection,
            read_connection,
            read_master_connection,
        }
    }

    pub(crate) fn get(
        &self,
        key: &str,
        chunk_id: u32,
    ) -> impl Future<Item = BlobstoreBytes, Error = Error> {
        let key = key.to_owned();
        let shard_id = self.shard(&key, chunk_id);
        let read_master_connection = self.read_master_connection[shard_id - 1].clone();

        SelectChunk::query(&self.read_connection[shard_id - 1], &key, &chunk_id).and_then(
            move |rows| match rows.into_iter().next() {
                Some((value,)) => Ok(BlobstoreBytes::from_bytes(value))
                    .into_future()
                    .left_future(),
                None => SelectChunk::query(&read_master_connection, &key, &chunk_id)
                    .and_then(move |rows| match rows.into_iter().next() {
                        Some((value,)) => Ok(BlobstoreBytes::from_bytes(value)),
                        None => Err(format_err!(
                            "Missing chunk with id {} shard {}",
                            chunk_id,
                            shard_id
                        )),
                    })
                    .right_future(),
            },
        )
    }

    pub(crate) fn put(
        &self,
        key: &str,
        chunk_id: u32,
        value: &[u8],
    ) -> impl Future<Item = (), Error = Error> {
        let shard_id = self.shard(key, chunk_id);

        InsertChunk::query(
            &self.write_connection[shard_id - 1],
            &[(&key, &chunk_id, &value)],
        )
        .map(|_| ())
    }

    fn shard(&self, key: &str, chunk_id: u32) -> usize {
        let mut hasher = XxHash32::with_seed(0);
        hasher.write(key.as_bytes());
        hasher.write_u32(chunk_id);
        ((hasher.finish() % self.shard_num.get() as u64) + 1) as usize
    }
}
