/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Adapters around Memcache to be transparently used as HgIdDataStore or HgIdHistoryStore.

use std::{mem::size_of, path::PathBuf, sync::Arc};

use anyhow::Result;
use bytes::Bytes;
use serde_derive::{Deserialize, Serialize};
use tracing::info_span;

use types::{Key, NodeInfo};

use crate::{
    datastore::{
        Delta, HgIdDataStore, HgIdMutableDeltaStore, Metadata, RemoteDataStore, StoreResult,
    },
    historystore::{HgIdHistoryStore, HgIdMutableHistoryStore, RemoteHistoryStore},
    localstore::LocalStore,
    remotestore::HgIdRemoteStore,
    types::StoreKey,
};

/// Type of blobs stored in Memcache.
///
/// Whenever this type is changed, the `CODE_VERSION` value must be incremented to avoid
/// incompatibilities.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub(crate) struct McData {
    pub key: Key,
    pub data: Bytes,
    pub metadata: Metadata,
}

/// Type of history info stored in Memcache.
///
/// Whenever this type is changed, the `CODE_VERSION` value must be incremented to avoid
/// incompatibilities.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub(crate) struct McHist {
    pub key: Key,
    pub nodeinfo: NodeInfo,
}

#[cfg(not(all(fbcode_build, target_os = "linux")))]
mod dummy {
    use super::*;

    use std::iter::empty;

    use configparser::config::ConfigSet;

    /// Dummy memcache client for when Mercurial is compiled outside of fbcode.
    pub struct MemcacheStore;

    impl MemcacheStore {
        pub fn new(_config: &ConfigSet) -> Result<Self> {
            Ok(MemcacheStore {})
        }

        pub(super) fn get_data_iter(&self, _key: &[Key]) -> impl Iterator<Item = Result<McData>> {
            empty()
        }

        pub(super) fn add_data(&self, _delta: &Delta, _metadata: &Metadata) {}

        pub(super) fn get_hist_iter(&self, _key: &[Key]) -> impl Iterator<Item = Result<McHist>> {
            empty()
        }

        pub(super) fn add_hist(&self, _key: &Key, _info: &NodeInfo) {}
    }
}

#[cfg(all(fbcode_build, target_os = "linux"))]
pub use crate::facebook::MemcacheStore;

#[cfg(not(all(fbcode_build, target_os = "linux")))]
pub use dummy::MemcacheStore;

impl HgIdDataStore for MemcacheStore {
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        Ok(StoreResult::NotFound(key))
    }

    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>> {
        Ok(StoreResult::NotFound(key))
    }
}

impl HgIdMutableDeltaStore for MemcacheStore {
    fn add(&self, delta: &Delta, metadata: &Metadata) -> Result<()> {
        self.add_data(delta, metadata);
        Ok(())
    }

    fn flush(&self) -> Result<Option<PathBuf>> {
        Ok(None)
    }
}

impl HgIdHistoryStore for MemcacheStore {
    fn get_node_info(&self, _key: &Key) -> Result<Option<NodeInfo>> {
        Ok(None)
    }
}

impl HgIdMutableHistoryStore for MemcacheStore {
    fn add(&self, key: &Key, info: &NodeInfo) -> Result<()> {
        self.add_hist(key, info);
        Ok(())
    }

    fn flush(&self) -> Result<Option<Vec<PathBuf>>> {
        Ok(None)
    }
}

impl LocalStore for MemcacheStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        Ok(keys.to_vec())
    }
}

impl HgIdRemoteStore for MemcacheStore {
    fn datastore(
        self: Arc<Self>,
        store: Arc<dyn HgIdMutableDeltaStore>,
    ) -> Arc<dyn RemoteDataStore> {
        Arc::new(MemcacheHgIdDataStore::new(self, store))
    }

    fn historystore(
        self: Arc<Self>,
        store: Arc<dyn HgIdMutableHistoryStore>,
    ) -> Arc<dyn RemoteHistoryStore> {
        Arc::new(MemcacheHgIdHistoryStore::new(self, store))
    }
}

struct MemcacheHgIdDataStore {
    store: Arc<dyn HgIdMutableDeltaStore>,
    memcache: Arc<MemcacheStore>,
}

impl MemcacheHgIdDataStore {
    pub fn new(memcache: Arc<MemcacheStore>, store: Arc<dyn HgIdMutableDeltaStore>) -> Self {
        Self { memcache, store }
    }
}

impl HgIdDataStore for MemcacheHgIdDataStore {
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        match self.prefetch(&[key.clone()]) {
            Ok(_) => self.store.get(key),
            Err(_) => Ok(StoreResult::NotFound(key)),
        }
    }

    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>> {
        match self.prefetch(&[key.clone()]) {
            Ok(_) => self.store.get_meta(key),
            Err(_) => Ok(StoreResult::NotFound(key)),
        }
    }
}

impl LocalStore for MemcacheHgIdDataStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.store.get_missing(keys)
    }
}

impl RemoteDataStore for MemcacheHgIdDataStore {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        let span = info_span!(
            "MemcacheHgIdDataStore::prefetch",
            key_count = keys.len(),
            hit_count = &0,
            size = &0
        );
        let _guard = span.enter();

        let mut hits = 0;
        let mut size = 0;

        let hgidkeys = keys
            .iter()
            .filter_map(|k| match k {
                StoreKey::HgId(k) => Some(k.clone()),
                StoreKey::Content(_, _) => None,
            })
            .collect::<Vec<_>>();

        for mcdata in self.memcache.get_data_iter(&hgidkeys) {
            if let Ok(mcdata) = mcdata {
                let metadata = mcdata.metadata;
                let delta = Delta {
                    data: mcdata.data,
                    base: None,
                    key: mcdata.key,
                };

                hits += 1;
                size += delta.data.len() + size_of::<Key>();

                self.store.add(&delta, &metadata)?;
            }
        }

        span.record("hit_count", &hits);
        span.record("size", &size);

        self.store.get_missing(keys)
    }

    fn upload(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        Ok(keys.to_vec())
    }
}

struct MemcacheHgIdHistoryStore {
    store: Arc<dyn HgIdMutableHistoryStore>,
    memcache: Arc<MemcacheStore>,
}

impl MemcacheHgIdHistoryStore {
    pub fn new(memcache: Arc<MemcacheStore>, store: Arc<dyn HgIdMutableHistoryStore>) -> Self {
        Self { memcache, store }
    }
}

impl HgIdHistoryStore for MemcacheHgIdHistoryStore {
    fn get_node_info(&self, key: &Key) -> Result<Option<NodeInfo>> {
        match self.prefetch(&[StoreKey::hgid(key.clone())]) {
            Ok(()) => self.store.get_node_info(key),
            Err(_) => Ok(None),
        }
    }
}

impl LocalStore for MemcacheHgIdHistoryStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.store.get_missing(keys)
    }
}

impl RemoteHistoryStore for MemcacheHgIdHistoryStore {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<()> {
        let span = info_span!(
            "MemcacheHgIdHistoryStore::prefetch",
            key_count = keys.len(),
            hit_count = &0,
            size = &0
        );
        let _guard = span.enter();

        let keys = keys
            .iter()
            .filter_map(|k| match k {
                StoreKey::HgId(k) => Some(k.clone()),
                StoreKey::Content(_, _) => None,
            })
            .collect::<Vec<_>>();

        let mut hits = 0;
        let mut size = 0;

        for mchist in self.memcache.get_hist_iter(&keys) {
            if let Ok(mchist) = mchist {
                self.store.add(&mchist.key, &mchist.nodeinfo)?;

                hits += 1;
                size += size_of::<McHist>();
            }
        }

        span.record("hit_count", &hits);
        span.record("size", &size);

        Ok(())
    }
}
