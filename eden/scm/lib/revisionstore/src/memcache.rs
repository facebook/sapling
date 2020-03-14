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
    datastore::{Delta, HgIdDataStore, HgIdMutableDeltaStore, Metadata, RemoteDataStore},
    historystore::{HgIdHistoryStore, HgIdMutableHistoryStore, RemoteHistoryStore},
    localstore::HgIdLocalStore,
    remotestore::HgIdRemoteStore,
};

/// Type of blobs stored in Memcache.
///
/// Whenever this type is changed, the `CODE_VERSION` value must be incremented to avoid
/// incompatibilities.
#[derive(Serialize, Deserialize)]
pub(crate) struct McData {
    pub key: Key,
    pub data: Bytes,
    pub metadata: Metadata,
}

/// Type of history info stored in Memcache.
///
/// Whenever this type is changed, the `CODE_VERSION` value must be incremented to avoid
/// incompatibilities.
#[derive(Serialize, Deserialize)]
pub(crate) struct McHist {
    pub key: Key,
    pub nodeinfo: NodeInfo,
}

#[cfg(not(fbcode_build))]
mod dummy {
    use super::*;

    use std::iter::empty;

    use configparser::config::ConfigSet;

    /// Dummy memcache client for when Mercurial is compiled outside of fbcode.
    #[derive(Clone)]
    pub struct MemcacheStore;

    impl MemcacheStore {
        pub fn new(_config: &ConfigSet) -> Result<Self> {
            Ok(MemcacheStore {})
        }

        pub(super) fn get_data_iter(&self, _key: &[Key]) -> impl Iterator<Item = Result<McData>> {
            empty()
        }

        pub(super) fn get_data(&self, _key: &Key) -> Result<Option<McData>> {
            Ok(None)
        }

        pub(super) fn add_data(&self, _delta: &Delta, _metadata: &Metadata) {}

        pub(super) fn get_hist_iter(&self, _key: &[Key]) -> impl Iterator<Item = Result<McHist>> {
            empty()
        }

        pub(super) fn get_hist(&self, _key: &Key) -> Result<Option<McHist>> {
            Ok(None)
        }

        pub(super) fn add_hist(&self, _key: &Key, _info: &NodeInfo) {}
    }
}

#[cfg(fbcode_build)]
pub use crate::facebook::MemcacheStore;

#[cfg(not(fbcode_build))]
pub use dummy::MemcacheStore;

impl HgIdDataStore for MemcacheStore {
    fn get(&self, key: &Key) -> Result<Option<Vec<u8>>> {
        self.get_data(key)
            .map(|opt| opt.map(|mcdata| mcdata.data.as_ref().to_vec()))
    }

    fn get_delta(&self, key: &Key) -> Result<Option<Delta>> {
        self.get_data(key).map(|opt| {
            opt.map(|mcdata| Delta {
                data: mcdata.data,
                base: None,
                key: mcdata.key,
            })
        })
    }

    fn get_delta_chain(&self, key: &Key) -> Result<Option<Vec<Delta>>> {
        self.get_delta(key).map(|opt| opt.map(|delta| vec![delta]))
    }

    fn get_meta(&self, key: &Key) -> Result<Option<Metadata>> {
        self.get_data(key)
            .map(|opt| opt.map(|mcdata| mcdata.metadata))
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
    fn get_node_info(&self, key: &Key) -> Result<Option<NodeInfo>> {
        self.get_hist(key)
            .map(|opt| opt.map(|mchist| mchist.nodeinfo))
    }
}

impl HgIdMutableHistoryStore for MemcacheStore {
    fn add(&self, key: &Key, info: &NodeInfo) -> Result<()> {
        self.add_hist(key, info);
        Ok(())
    }

    fn flush(&self) -> Result<Option<PathBuf>> {
        Ok(None)
    }
}

impl HgIdLocalStore for MemcacheStore {
    fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>> {
        Ok(keys.to_vec())
    }
}

impl HgIdRemoteStore for MemcacheStore {
    fn datastore(&self, store: Arc<dyn HgIdMutableDeltaStore>) -> Arc<dyn RemoteDataStore> {
        Arc::new(MemcacheHgIdDataStore::new(self.clone(), store))
    }

    fn historystore(&self, store: Arc<dyn HgIdMutableHistoryStore>) -> Arc<dyn RemoteHistoryStore> {
        Arc::new(MemcacheHgIdHistoryStore::new(self.clone(), store))
    }
}

struct MemcacheHgIdDataStore {
    store: Arc<dyn HgIdMutableDeltaStore>,
    memcache: MemcacheStore,
}

impl MemcacheHgIdDataStore {
    pub fn new(memcache: MemcacheStore, store: Arc<dyn HgIdMutableDeltaStore>) -> Self {
        Self { memcache, store }
    }
}

impl HgIdDataStore for MemcacheHgIdDataStore {
    fn get(&self, key: &Key) -> Result<Option<Vec<u8>>> {
        self.memcache.get(key)
    }

    fn get_delta(&self, key: &Key) -> Result<Option<Delta>> {
        self.memcache.get_delta(key)
    }

    fn get_delta_chain(&self, key: &Key) -> Result<Option<Vec<Delta>>> {
        self.memcache.get_delta_chain(key)
    }

    fn get_meta(&self, key: &Key) -> Result<Option<Metadata>> {
        self.memcache.get_meta(key)
    }
}

impl HgIdLocalStore for MemcacheHgIdDataStore {
    fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>> {
        self.store.get_missing(keys)
    }
}

impl RemoteDataStore for MemcacheHgIdDataStore {
    fn prefetch(&self, keys: &[Key]) -> Result<()> {
        let span = info_span!(
            "MemcacheHgIdDataStore::prefetch",
            key_count = keys.len(),
            hit_count = &0,
            size = &0
        );
        let _guard = span.enter();

        let mut hits = 0;
        let mut size = 0;

        for mcdata in self.memcache.get_data_iter(keys) {
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

        span.record("hits", &hits);
        span.record("size", &size);

        Ok(())
    }
}

struct MemcacheHgIdHistoryStore {
    store: Arc<dyn HgIdMutableHistoryStore>,
    memcache: MemcacheStore,
}

impl MemcacheHgIdHistoryStore {
    pub fn new(memcache: MemcacheStore, store: Arc<dyn HgIdMutableHistoryStore>) -> Self {
        Self { memcache, store }
    }
}

impl HgIdHistoryStore for MemcacheHgIdHistoryStore {
    fn get_node_info(&self, key: &Key) -> Result<Option<NodeInfo>> {
        self.memcache.get_node_info(key)
    }
}

impl HgIdLocalStore for MemcacheHgIdHistoryStore {
    fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>> {
        self.store.get_missing(keys)
    }
}

impl RemoteHistoryStore for MemcacheHgIdHistoryStore {
    fn prefetch(&self, keys: &[Key]) -> Result<()> {
        let span = info_span!(
            "MemcacheHgIdHistoryStore::prefetch",
            key_count = keys.len(),
            hit_count = &0,
            size = &0
        );
        let _guard = span.enter();

        let mut hits = 0;
        let mut size = 0;

        for mchist in self.memcache.get_hist_iter(keys) {
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
