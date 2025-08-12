/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use edenapi_types::FileEntry;
use edenapi_types::TreeEntry;
use minibytes::Bytes;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use types::Key;

pub use crate::Metadata;
use crate::localstore::LocalStore;
use crate::types::ContentHash;
use crate::types::StoreKey;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Delta {
    pub data: Bytes,
    #[serde(with = "types::serde_with::key::tuple")]
    pub base: Option<Key>,
    #[serde(with = "types::serde_with::key::tuple")]
    pub key: Key,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StoreResult<T> {
    Found(T),
    NotFound(StoreKey),
}

impl<T> From<StoreResult<T>> for Option<T> {
    fn from(v: StoreResult<T>) -> Self {
        match v {
            StoreResult::Found(v) => Some(v),
            StoreResult::NotFound(_) => None,
        }
    }
}

pub trait HgIdDataStore: LocalStore + Send + Sync {
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>>;
    fn refresh(&self) -> Result<()>;
}

/// The `RemoteDataStore` trait indicates that data can fetched over the network. Care must be
/// taken to avoid serially fetching data and instead data should be fetched in bulk via the
/// `prefetch` API.
pub trait RemoteDataStore: HgIdDataStore + Send + Sync {
    /// Attempt to bring the data corresponding to the passed in keys to a local store.
    ///
    /// When implemented on a pure remote store, like the `SaplingRemoteApi`, the method will always fetch
    /// everything that was asked. On a higher level store, such as the `ContentStore`, this will
    /// avoid fetching data that is already present locally.
    fn prefetch(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>>;

    /// Send all the blobs referenced by the keys to the remote store.
    fn upload(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>>;
}

pub trait HgIdMutableDeltaStore: HgIdDataStore + Send + Sync {
    fn add(&self, delta: &Delta, metadata: &Metadata) -> Result<()>;
    fn flush(&self) -> Result<Option<Vec<PathBuf>>>;

    fn add_file(&self, entry: &FileEntry) -> Result<()> {
        let delta = Delta {
            data: entry.data()?,
            base: None,
            key: entry.key().clone(),
        };
        self.add(&delta, entry.metadata()?)
    }

    fn add_tree(&self, entry: &TreeEntry) -> Result<()> {
        let delta = Delta {
            data: entry.data()?,
            base: None,
            key: entry.key().clone(),
        };
        self.add(
            &delta,
            &Metadata {
                flags: None,
                size: None,
            },
        )
    }
}

pub trait LegacyStore: HgIdMutableDeltaStore + RemoteDataStore + Send + Sync {
    fn get_file_content(&self, key: &Key) -> Result<Option<Bytes>>;
    fn get_shared_mutable(&self) -> Arc<dyn HgIdMutableDeltaStore>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentMetadata {
    pub size: usize,
    pub hash: ContentHash,
    pub is_binary: bool,
}

/// Implement `HgIdDataStore` for all types that can be `Deref` into a `HgIdDataStore`. This includes all
/// the smart pointers like `Box`, `Rc`, `Arc`.
impl<T: HgIdDataStore + ?Sized, U: Deref<Target = T> + Send + Sync> HgIdDataStore for U {
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        T::get(self, key)
    }

    /// Tell the underlying stores that there may be new data on disk.
    fn refresh(&self) -> Result<()> {
        T::refresh(self)
    }
}

/// Implement `RemoteDataStore` for all types that can be `Deref` into a `RemoteDataStore`. This
/// includes all the smart pointers like `Box`, `Rc`, `Arc`.
impl<T: RemoteDataStore + ?Sized, U: Deref<Target = T> + Send + Sync> RemoteDataStore for U {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        T::prefetch(self, keys)
    }

    fn upload(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        T::upload(self, keys)
    }
}

impl<T: HgIdMutableDeltaStore + ?Sized, U: Deref<Target = T> + Send + Sync> HgIdMutableDeltaStore
    for U
{
    fn add(&self, delta: &Delta, metadata: &Metadata) -> Result<()> {
        T::add(self, delta, metadata)
    }

    fn flush(&self) -> Result<Option<Vec<PathBuf>>> {
        T::flush(self)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use revisionstore_types::InternalMetadata;

    use super::*;

    fn roundtrip_meta_serialize(meta: &Metadata) {
        let mut buf = vec![];
        InternalMetadata {
            api: *meta,
            uncompressed: false,
        }
        .write(&mut buf)
        .expect("write");
        let read_meta = InternalMetadata::read(&mut Cursor::new(&buf))
            .expect("meta")
            .api;
        assert!(*meta == read_meta);
    }

    #[test]
    fn test_metadata_serialize() {
        roundtrip_meta_serialize(&Metadata {
            size: None,
            flags: None,
        });
        roundtrip_meta_serialize(&Metadata {
            size: Some(5),
            flags: None,
        });
        roundtrip_meta_serialize(&Metadata {
            size: Some(0),
            flags: Some(12),
        });
        roundtrip_meta_serialize(&Metadata {
            size: Some(1000),
            flags: Some(12),
        });
        roundtrip_meta_serialize(&Metadata {
            size: Some(234214134),
            flags: Some(9879489),
        });
    }
}
