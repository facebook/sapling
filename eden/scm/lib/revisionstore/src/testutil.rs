/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{collections::HashMap, path::Path, sync::Arc};

use anyhow::{Context, Error, Result};
use async_trait::async_trait;
use bytes::Bytes;
use futures::prelude::*;

use configparser::config::ConfigSet;
use edenapi::{EdenApi, EdenApiError, Fetch, ProgressCallback, RepoName, ResponseMeta, Stats};
use edenapi_types::{DataEntry, HistoryEntry};
use types::{HgId, Key, NodeInfo, Parents, RepoPathBuf};

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
    fn datastore(
        self: Arc<Self>,
        store: Arc<dyn HgIdMutableDeltaStore>,
    ) -> Arc<dyn RemoteDataStore> {
        assert!(self.data.is_some());

        Arc::new(FakeRemoteDataStore {
            store,
            map: self.data.as_ref().unwrap().clone(),
        })
    }

    fn historystore(
        self: Arc<Self>,
        store: Arc<dyn HgIdMutableHistoryStore>,
    ) -> Arc<dyn RemoteHistoryStore> {
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
                StoreKey::Content(_, _) => continue,
            }
        }

        Ok(())
    }

    fn upload(&self, _keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        unimplemented!()
    }
}

impl HgIdDataStore for FakeRemoteDataStore {
    fn get(&self, key: &Key) -> Result<Option<Vec<u8>>> {
        let missing = self.translate_lfs_missing(&[StoreKey::hgid(key.clone())])?;
        match self.prefetch(&missing) {
            Err(_) => Ok(None),
            Ok(()) => self.store.get(key),
        }
    }

    fn get_meta(&self, key: &Key) -> Result<Option<Metadata>> {
        let missing = self.translate_lfs_missing(&[StoreKey::hgid(key.clone())])?;
        match self.prefetch(&missing) {
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
                StoreKey::Content(_, _) => continue,
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
}

#[async_trait]
impl EdenApi for FakeEdenApi {
    async fn health(&self) -> Result<ResponseMeta, EdenApiError> {
        Ok(ResponseMeta::default())
    }

    async fn files(
        &self,
        _repo: RepoName,
        keys: Vec<Key>,
        _progress: Option<ProgressCallback>,
    ) -> Result<Fetch<DataEntry>, EdenApiError> {
        let entries = keys
            .into_iter()
            .map(|key| {
                let data = self.map.get(&key).context("Not found")?.clone();
                let parents = Parents::default();
                Ok(DataEntry::new(key, data, parents))
            })
            .collect::<Vec<_>>();

        Ok(Fetch {
            meta: vec![ResponseMeta::default()],
            entries: Box::pin(stream::iter(entries)),
            stats: Box::pin(future::ok(Stats::default())),
        })
    }

    async fn history(
        &self,
        _repo: RepoName,
        _keys: Vec<Key>,
        _length: Option<u32>,
        _progress: Option<ProgressCallback>,
    ) -> Result<Fetch<HistoryEntry>, EdenApiError> {
        unimplemented!()
    }

    async fn trees(
        &self,
        _repo: RepoName,
        _keys: Vec<Key>,
        _progress: Option<ProgressCallback>,
    ) -> Result<Fetch<DataEntry>, EdenApiError> {
        unimplemented!()
    }

    async fn complete_trees(
        &self,
        _repo: RepoName,
        _rootdir: RepoPathBuf,
        _mfnodes: Vec<HgId>,
        _basemfnodes: Vec<HgId>,
        _depth: Option<usize>,
        _progress: Option<ProgressCallback>,
    ) -> Result<Fetch<DataEntry>, EdenApiError> {
        unimplemented!()
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

    config.set("lfs", "moveafterupload", Some("true"), &Default::default());

    config
}
