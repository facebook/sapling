/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use edenapi_types::FileEntry;
use edenapi_types::TreeEntry;
use minibytes::Bytes;
use regex::Regex;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use types::Key;
use types::RepoPathBuf;

use crate::fetch_logger::FetchLogger;
use crate::localstore::LocalStore;
use crate::types::ContentHash;
use crate::types::StoreKey;
pub use crate::Metadata;
use crate::RepackLocation;

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
    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>>;
    fn refresh(&self) -> Result<()>;
}

/// The `RemoteDataStore` trait indicates that data can fetched over the network. Care must be
/// taken to avoid serially fetching data and instead data should be fetched in bulk via the
/// `prefetch` API.
pub trait RemoteDataStore: HgIdDataStore + Send + Sync {
    /// Attempt to bring the data corresponding to the passed in keys to a local store.
    ///
    /// When implemented on a pure remote store, like the `EdenApi`, the method will always fetch
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
            data: entry.data()?.into(),
            base: None,
            key: entry.key().clone(),
        };
        self.add(&delta, entry.metadata()?)
    }

    fn add_tree(&self, entry: &TreeEntry) -> Result<()> {
        let delta = Delta {
            data: entry.data()?.into(),
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
    fn add_pending(
        &self,
        key: &Key,
        data: Bytes,
        meta: Metadata,
        location: RepackLocation,
    ) -> Result<()>;
    fn commit_pending(&self, location: RepackLocation) -> Result<Option<Vec<PathBuf>>>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentMetadata {
    pub size: usize,
    pub hash: ContentHash,
    pub is_binary: bool,
}

/// The `ContentDataStore` is intended for pure content only stores
///
/// Overtime, this new trait will replace the need for the `HgIdDataStore`, for now, only the LFS
/// store can implement it. Non content only stores could implement it, but the cost of the
/// `metadata` method will become linear over the blob size, reducing the benefit. A caching layer
/// will need to be put in place to avoid this.
pub trait ContentDataStore: Send + Sync {
    /// Read the blob from the store, the blob returned is the pure blob and will not contain any
    /// Mercurial copy_from header.
    fn blob(&self, key: StoreKey) -> Result<StoreResult<Bytes>>;

    /// Read the blob metadata from the store.
    fn metadata(&self, key: StoreKey) -> Result<StoreResult<ContentMetadata>>;
    // XXX: Add write operations.
}

/// Implement `HgIdDataStore` for all types that can be `Deref` into a `HgIdDataStore`. This includes all
/// the smart pointers like `Box`, `Rc`, `Arc`.
impl<T: HgIdDataStore + ?Sized, U: Deref<Target = T> + Send + Sync> HgIdDataStore for U {
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        T::get(self, key)
    }

    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>> {
        T::get_meta(self, key)
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

/// Implement `ContentDataStore` for all types that can be `Deref` into a `ContentDataStore`.
impl<T: ContentDataStore + ?Sized, U: Deref<Target = T> + Send + Sync> ContentDataStore for U {
    fn blob(&self, key: StoreKey) -> Result<StoreResult<Bytes>> {
        T::blob(self, key)
    }

    fn metadata(&self, key: StoreKey) -> Result<StoreResult<ContentMetadata>> {
        T::metadata(self, key)
    }
}

pub struct ReportingRemoteDataStore {
    store: Box<dyn RemoteDataStore>,
    logger: FetchLogger,
}

impl ReportingRemoteDataStore {
    pub fn new(store: Box<dyn RemoteDataStore>, filter: Option<Regex>) -> Self {
        Self {
            store,
            logger: FetchLogger::new(filter),
        }
    }

    pub fn take_seen(&self) -> HashSet<RepoPathBuf> {
        self.logger.take_seen()
    }

    fn report_keys(&self, keys: &[StoreKey]) {
        self.logger.report_store_keys(keys.iter())
    }
}

impl LocalStore for ReportingRemoteDataStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        Ok(keys.to_vec())
    }
}

impl HgIdDataStore for ReportingRemoteDataStore {
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        self.report_keys(&[key.clone()]);
        self.store.get(key)
    }
    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>> {
        self.report_keys(&[key.clone()]);
        self.store.get_meta(key)
    }

    fn refresh(&self) -> Result<()> {
        self.store.refresh()
    }
}

impl RemoteDataStore for ReportingRemoteDataStore {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.report_keys(keys);
        self.store.prefetch(keys)
    }

    fn upload(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.store.upload(keys)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn roundtrip_meta_serialize(meta: &Metadata) {
        let mut buf = vec![];
        meta.write(&mut buf).expect("write");
        let read_meta = Metadata::read(&mut Cursor::new(&buf)).expect("meta");
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
