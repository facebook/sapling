/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Adapters around Memcache to be transparently used as DataStore or HistoryStore.

use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use bytes::Bytes;
use serde_derive::{Deserialize, Serialize};

use types::{Key, NodeInfo};

use crate::{
    datastore::{DataStore, Delta, Metadata, MutableDeltaStore, RemoteDataStore},
    historystore::{HistoryStore, MutableHistoryStore, RemoteHistoryStore},
    localstore::LocalStore,
    remotestore::RemoteStore,
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

    /// Dummy memcache client for when Mercurial is compiled outside of fbcode.
    #[derive(Clone)]
    pub struct MemcacheStore;

    impl MemcacheStore {
        pub fn new() -> Result<Self> {
            Ok(MemcacheStore {})
        }

        pub(super) fn get_data(&self, _key: &Key) -> Result<Option<McData>> {
            Ok(None)
        }

        pub(super) fn add_data(&self, _delta: &Delta, _metadata: &Metadata) {}

        pub(super) fn get_hist(&self, _key: &Key) -> Result<Option<McHist>> {
            Ok(None)
        }

        pub(super) fn add_hist(&self, _key: &Key, _info: &NodeInfo) -> Result<()> {
            Ok(())
        }
    }
}

#[cfg(fbcode_build)]
pub use crate::facebook::MemcacheStore;

#[cfg(not(fbcode_build))]
pub use dummy::MemcacheStore;

impl DataStore for MemcacheStore {
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

impl MutableDeltaStore for MemcacheStore {
    fn add(&self, delta: &Delta, metadata: &Metadata) -> Result<()> {
        self.add_data(delta, metadata);
        Ok(())
    }

    fn flush(&self) -> Result<Option<PathBuf>> {
        Ok(None)
    }
}

impl HistoryStore for MemcacheStore {
    fn get_node_info(&self, key: &Key) -> Result<Option<NodeInfo>> {
        self.get_hist(key)
            .map(|opt| opt.map(|mchist| mchist.nodeinfo))
    }
}

impl MutableHistoryStore for MemcacheStore {
    fn add(&self, key: &Key, info: &NodeInfo) -> Result<()> {
        self.add_hist(key, info)
    }

    fn flush(&self) -> Result<Option<PathBuf>> {
        Ok(None)
    }
}

impl LocalStore for MemcacheStore {
    fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>> {
        Ok(keys.to_vec())
    }
}

impl RemoteStore for MemcacheStore {
    fn datastore(&self, store: Box<dyn MutableDeltaStore>) -> Arc<dyn RemoteDataStore> {
        Arc::new(MemcacheDataStore::new(self.clone(), store))
    }

    fn historystore(&self, store: Box<dyn MutableHistoryStore>) -> Arc<dyn RemoteHistoryStore> {
        Arc::new(MemcacheHistoryStore::new(self.clone(), store))
    }
}

struct MemcacheDataStore {
    store: Box<dyn MutableDeltaStore>,
    memcache: MemcacheStore,
}

impl MemcacheDataStore {
    pub fn new(memcache: MemcacheStore, store: Box<dyn MutableDeltaStore>) -> Self {
        Self { memcache, store }
    }
}

impl DataStore for MemcacheDataStore {
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

impl LocalStore for MemcacheDataStore {
    fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>> {
        self.store.get_missing(keys)
    }
}

impl RemoteDataStore for MemcacheDataStore {
    fn prefetch(&self, keys: &[Key]) -> Result<()> {
        for k in keys {
            if let Some(mcdata) = self.memcache.get_data(k)? {
                let metadata = mcdata.metadata;
                let delta = Delta {
                    data: mcdata.data,
                    base: None,
                    key: mcdata.key,
                };

                self.store.add(&delta, &metadata)?;
            };
        }

        Ok(())
    }
}

struct MemcacheHistoryStore {
    store: Box<dyn MutableHistoryStore>,
    memcache: MemcacheStore,
}

impl MemcacheHistoryStore {
    pub fn new(memcache: MemcacheStore, store: Box<dyn MutableHistoryStore>) -> Self {
        Self { memcache, store }
    }
}

impl HistoryStore for MemcacheHistoryStore {
    fn get_node_info(&self, key: &Key) -> Result<Option<NodeInfo>> {
        self.memcache.get_node_info(key)
    }
}

impl LocalStore for MemcacheHistoryStore {
    fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>> {
        self.store.get_missing(keys)
    }
}

impl RemoteHistoryStore for MemcacheHistoryStore {
    fn prefetch(&self, keys: &[Key]) -> Result<()> {
        for k in keys {
            if let Some(nodeinfo) = self.get_node_info(k)? {
                self.store.add(k, &nodeinfo)?;
            }
        }

        Ok(())
    }
}
