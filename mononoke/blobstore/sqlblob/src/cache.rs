/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::mem::transmute;
use std::sync::Arc;

use anyhow::{bail, Error, Result};
use cloned::cloned;
use fbthrift::compact_protocol;
use futures::prelude::*;

use cacheblob::{CacheOps, CacheOpsUtil};
use mononoke_types::BlobstoreBytes;
use sqlblob_thrift::{DataCacheEntry, InChunk};

use crate::{i32_to_non_zero_usize, DataEntry};

pub(crate) trait CacheTranslator {
    type Key;
    type Value;

    fn to_cache(&self, value: &Self::Value) -> BlobstoreBytes;

    fn from_cache(&self, bytes: BlobstoreBytes) -> Result<Self::Value>;

    fn cache_key(&self, key: &Self::Key) -> String;
}

pub(crate) struct SqlblobCacheOps<T, C> {
    cache: Arc<C>,
    translator: T,
}

impl<T: Clone, C> Clone for SqlblobCacheOps<T, C> {
    fn clone(&self) -> Self {
        Self {
            cache: self.cache.clone(),
            translator: self.translator.clone(),
        }
    }
}

impl<T, C> SqlblobCacheOps<T, C>
where
    T: CacheTranslator + Clone + Send + 'static,
    T::Value: Send + 'static,
    C: CacheOps,
{
    pub(crate) fn new(cache: Arc<C>, translator: T) -> Self {
        Self { cache, translator }
    }

    pub(crate) fn get(
        &self,
        key: &T::Key,
    ) -> impl Future<Item = Option<T::Value>, Error = Error> + Send {
        cloned!(self.translator);
        let key = translator.cache_key(key);

        CacheOpsUtil::get(&self.cache, &key)
            .and_then(move |maybe| maybe.map(|value| translator.from_cache(value)).transpose())
    }

    pub(crate) fn put(&self, key: &T::Key, value: T::Value) -> T::Value {
        {
            let key = self.translator.cache_key(key);
            let value = self.translator.to_cache(&value);
            tokio::spawn(self.cache.put(&key, value));
        }
        value
    }
}

#[derive(Clone)]
pub(crate) struct DataCacheTranslator {}

impl DataCacheTranslator {
    pub(crate) fn new() -> Self {
        Self {}
    }
}

impl CacheTranslator for DataCacheTranslator {
    type Key = String;
    type Value = DataEntry;

    fn to_cache(&self, value: &Self::Value) -> BlobstoreBytes {
        let thrift_val = match value {
            DataEntry::Data(data) => DataCacheEntry::data(
                data.as_bytes()
                    .iter()
                    .map(|b| unsafe { transmute::<u8, i8>(*b) })
                    .collect(),
            ),
            DataEntry::InChunk(num_of_chunks) => {
                DataCacheEntry::in_chunk(InChunk::num_of_chunks(num_of_chunks.get() as i32))
            }
        };

        BlobstoreBytes::from_bytes(compact_protocol::serialize(&thrift_val))
    }

    fn from_cache(&self, bytes: BlobstoreBytes) -> Result<Self::Value> {
        match compact_protocol::deserialize(bytes.into_bytes()) {
            Ok(DataCacheEntry::in_chunk(InChunk::num_of_chunks(num_of_chunks))) => {
                match i32_to_non_zero_usize(num_of_chunks) {
                    None => bail!("DataCacheEntry::in_chunk contains an invalid num of chunks"),
                    Some(num_of_chunks) => Ok(DataEntry::InChunk(num_of_chunks)),
                }
            }
            Ok(DataCacheEntry::data(data)) => Ok(DataEntry::Data(BlobstoreBytes::from_bytes(
                data.into_iter()
                    .map(|b| unsafe { transmute::<i8, u8>(b) })
                    .collect::<Vec<_>>(),
            ))),
            Err(_)
            | Ok(DataCacheEntry::UnknownField(_))
            | Ok(DataCacheEntry::in_chunk(InChunk::UnknownField(_))) => {
                bail!("Failed to deserialize DataCacheEntry")
            }
        }
    }

    fn cache_key(&self, key: &Self::Key) -> String {
        format!("{}", key)
    }
}

#[derive(Clone)]
pub(crate) struct ChunkCacheTranslator {}

impl ChunkCacheTranslator {
    pub(crate) fn new() -> Self {
        Self {}
    }
}

impl CacheTranslator for ChunkCacheTranslator {
    type Key = (String, u32);
    type Value = BlobstoreBytes;

    fn to_cache(&self, value: &Self::Value) -> BlobstoreBytes {
        value.clone()
    }

    fn from_cache(&self, bytes: BlobstoreBytes) -> Result<Self::Value> {
        Ok(bytes)
    }

    fn cache_key(&self, &(ref key, ref chunk_id): &Self::Key) -> String {
        format!("{}.{}", key, chunk_id)
    }
}
