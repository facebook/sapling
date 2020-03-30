/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{collections::HashMap, path::Path, sync::Arc, time::Duration};

use anyhow::{Error, Result};
use bytes::Bytes;

use configparser::config::ConfigSet;
use edenapi::{ApiResult, DownloadStats, EdenApi, ProgressFn};
use types::{HgId, HistoryEntry, Key, NodeInfo, RepoPathBuf};

use crate::{
    datastore::{Delta, HgIdDataStore, HgIdMutableDeltaStore, Metadata, RemoteDataStore},
    historystore::{HgIdHistoryStore, HgIdMutableHistoryStore, RemoteHistoryStore},
    localstore::LocalStore,
    remotestore::HgIdRemoteStore,
    types::StoreKey,
};

pub fn delta(data: &str, base: Option<Key>, key: Key) -> Delta {
    Delta {
        data: Bytes::copy_from_slice(data.as_bytes()),
        base,
        key,
    }
}

pub struct FakeHgIdRemoteStore {
    data: Option<HashMap<Key, (Bytes, Option<u64>)>>,
    hist: Option<HashMap<Key, NodeInfo>>,
}

impl FakeHgIdRemoteStore {
    pub fn new() -> FakeHgIdRemoteStore {
        Self {
            data: None,
            hist: None,
        }
    }

    pub fn data(&mut self, map: HashMap<Key, (Bytes, Option<u64>)>) {
        self.data = Some(map)
    }

    pub fn hist(&mut self, map: HashMap<Key, NodeInfo>) {
        self.hist = Some(map)
    }
}

impl HgIdRemoteStore for FakeHgIdRemoteStore {
    fn datastore(&self, store: Arc<dyn HgIdMutableDeltaStore>) -> Arc<dyn RemoteDataStore> {
        assert!(self.data.is_some());

        Arc::new(FakeRemoteDataStore {
            store,
            map: self.data.as_ref().unwrap().clone(),
        })
    }

    fn historystore(&self, store: Arc<dyn HgIdMutableHistoryStore>) -> Arc<dyn RemoteHistoryStore> {
        assert!(self.hist.is_some());

        Arc::new(FakeRemoteHistoryStore {
            store,
            map: self.hist.as_ref().unwrap().clone(),
        })
    }
}

struct FakeRemoteDataStore {
    store: Arc<dyn HgIdMutableDeltaStore>,
    map: HashMap<Key, (Bytes, Option<u64>)>,
}

impl RemoteDataStore for FakeRemoteDataStore {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<()> {
        for k in keys {
            match k {
                StoreKey::HgId(k) => {
                    let (data, flags) = self.map.get(&k).ok_or_else(|| Error::msg("Not found"))?;
                    let delta = Delta {
                        data: data.clone(),
                        base: None,
                        key: k.clone(),
                    };
                    self.store.add(
                        &delta,
                        &Metadata {
                            size: Some(data.len() as u64),
                            flags: *flags,
                        },
                    )?;
                }
                StoreKey::Content(_) => continue,
            }
        }

        Ok(())
    }
}

impl HgIdDataStore for FakeRemoteDataStore {
    fn get(&self, _key: &Key) -> Result<Option<Vec<u8>>> {
        unreachable!();
    }

    fn get_delta(&self, key: &Key) -> Result<Option<Delta>> {
        match self.prefetch(&[StoreKey::hgid(key.clone())]) {
            Err(_) => Ok(None),
            Ok(()) => self.store.get_delta(key),
        }
    }

    fn get_delta_chain(&self, key: &Key) -> Result<Option<Vec<Delta>>> {
        match self.prefetch(&[StoreKey::hgid(key.clone())]) {
            Err(_) => Ok(None),
            Ok(()) => self.store.get_delta_chain(key),
        }
    }

    fn get_meta(&self, key: &Key) -> Result<Option<Metadata>> {
        match self.prefetch(&[StoreKey::hgid(key.clone())]) {
            Err(_) => Ok(None),
            Ok(()) => self.store.get_meta(key),
        }
    }
}

impl LocalStore for FakeRemoteDataStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.store.get_missing(keys)
    }
}

struct FakeRemoteHistoryStore {
    store: Arc<dyn HgIdMutableHistoryStore>,
    map: HashMap<Key, NodeInfo>,
}

impl RemoteHistoryStore for FakeRemoteHistoryStore {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<()> {
        for k in keys {
            match k {
                StoreKey::HgId(k) => self
                    .store
                    .add(&k, self.map.get(&k).ok_or_else(|| Error::msg("Not found"))?)?,
                StoreKey::Content(_) => continue,
            }
        }

        Ok(())
    }
}

impl HgIdHistoryStore for FakeRemoteHistoryStore {
    fn get_node_info(&self, key: &Key) -> Result<Option<NodeInfo>> {
        match self.prefetch(&[StoreKey::hgid(key.clone())]) {
            Err(_) => Ok(None),
            Ok(()) => self.store.get_node_info(key),
        }
    }
}

impl LocalStore for FakeRemoteHistoryStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.store.get_missing(keys)
    }
}

struct FakeEdenApi {
    map: HashMap<Key, Bytes>,
}

impl FakeEdenApi {
    pub fn new(map: HashMap<Key, Bytes>) -> FakeEdenApi {
        Self { map }
    }

    fn fake_downloadstats(&self) -> DownloadStats {
        DownloadStats {
            downloaded: 0,
            uploaded: 0,
            requests: 0,
            time: Duration::from_secs(0),
            latency: Duration::from_secs(0),
        }
    }
}

impl EdenApi for FakeEdenApi {
    fn health_check(&self) -> ApiResult<()> {
        Ok(())
    }

    fn hostname(&self) -> ApiResult<String> {
        Ok("test".to_string())
    }

    fn get_files(
        &self,
        keys: Vec<Key>,
        _progress: Option<ProgressFn>,
    ) -> ApiResult<(Box<dyn Iterator<Item = (Key, Bytes)>>, DownloadStats)> {
        let stats = self.fake_downloadstats();
        let iter = keys
            .into_iter()
            .map(|key| {
                self.map
                    .get(&key)
                    .ok_or_else(|| "Not found".into())
                    .map(|data| (key, data.clone()))
            })
            .collect::<ApiResult<Vec<(Key, Bytes)>>>()?;
        Ok((Box::new(iter.into_iter()), stats))
    }

    fn get_history(
        &self,
        _keys: Vec<Key>,
        _max_depth: Option<u32>,
        _progress: Option<ProgressFn>,
    ) -> ApiResult<(Box<dyn Iterator<Item = HistoryEntry>>, DownloadStats)> {
        unreachable!();
    }

    fn get_trees(
        &self,
        _keys: Vec<Key>,
        _progress: Option<ProgressFn>,
    ) -> ApiResult<(Box<dyn Iterator<Item = (Key, Bytes)>>, DownloadStats)> {
        unreachable!();
    }

    fn prefetch_trees(
        &self,
        _rootdir: RepoPathBuf,
        _mfnodes: Vec<HgId>,
        _basemfnodes: Vec<HgId>,
        _depth: Option<usize>,
        _progress: Option<ProgressFn>,
    ) -> ApiResult<(Box<dyn Iterator<Item = (Key, Bytes)>>, DownloadStats)> {
        unreachable!();
    }
}

pub fn fake_edenapi(map: HashMap<Key, Bytes>) -> Arc<dyn EdenApi> {
    Arc::new(FakeEdenApi::new(map))
}

pub fn make_config(dir: impl AsRef<Path>) -> ConfigSet {
    let mut config = ConfigSet::new();

    config.set(
        "remotefilelog",
        "reponame",
        Some("test"),
        &Default::default(),
    );
    config.set(
        "remotefilelog",
        "cachepath",
        Some(dir.as_ref().to_str().unwrap()),
        &Default::default(),
    );

    config.set(
        "remotefilelog",
        "cachekey",
        Some("cca:hg:rust_unittest"),
        &Default::default(),
    );

    config
}

pub fn make_lfs_config(dir: impl AsRef<Path>) -> ConfigSet {
    let mut config = make_config(dir);

    config.set(
        "lfs",
        "url",
        Some("https://mononoke-lfs.internal.tfbnw.net/ovrsource"),
        &Default::default(),
    );

    config.set(
        "experimental",
        "lfs.user-agent",
        Some("mercurial/revisionstore/unittests"),
        &Default::default(),
    );

    config.set("lfs", "threshold", Some("4"), &Default::default());

    config.set("remotefilelog", "lfs", Some("true"), &Default::default());

    config
}
