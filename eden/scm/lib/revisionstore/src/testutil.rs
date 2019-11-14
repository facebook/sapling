/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{collections::HashMap, sync::Arc, time::Duration};

use bytes::Bytes;
use failure::{err_msg, Fallible as Result};

use edenapi::{ApiResult, DownloadStats, EdenApi, ProgressFn};
use types::{HgId, HistoryEntry, Key, NodeInfo, RepoPathBuf};

use crate::{
    datastore::{DataStore, Delta, Metadata, MutableDeltaStore, RemoteDataStore},
    historystore::{HistoryStore, MutableHistoryStore, RemoteHistoryStore},
    localstore::LocalStore,
    remotestore::RemoteStore,
};

pub fn delta(data: &str, base: Option<Key>, key: Key) -> Delta {
    Delta {
        data: Bytes::from(data),
        base,
        key,
    }
}

pub struct FakeRemoteStore {
    data: Option<HashMap<Key, Bytes>>,
    hist: Option<HashMap<Key, NodeInfo>>,
}

impl FakeRemoteStore {
    pub fn new() -> FakeRemoteStore {
        Self {
            data: None,
            hist: None,
        }
    }

    pub fn data(&mut self, map: HashMap<Key, Bytes>) {
        self.data = Some(map)
    }

    pub fn hist(&mut self, map: HashMap<Key, NodeInfo>) {
        self.hist = Some(map)
    }
}

impl RemoteStore for FakeRemoteStore {
    fn datastore(&self, store: Box<dyn MutableDeltaStore>) -> Arc<dyn RemoteDataStore> {
        assert!(self.data.is_some());

        Arc::new(FakeRemoteDataStore {
            store,
            map: self.data.as_ref().unwrap().clone(),
        })
    }

    fn historystore(&self, store: Box<dyn MutableHistoryStore>) -> Arc<dyn RemoteHistoryStore> {
        assert!(self.hist.is_some());

        Arc::new(FakeRemoteHistoryStore {
            store,
            map: self.hist.as_ref().unwrap().clone(),
        })
    }
}

struct FakeRemoteDataStore {
    store: Box<dyn MutableDeltaStore>,
    map: HashMap<Key, Bytes>,
}

impl RemoteDataStore for FakeRemoteDataStore {
    fn prefetch(&self, keys: Vec<Key>) -> Result<()> {
        for k in keys {
            let data = self.map.get(&k).ok_or(err_msg("Not found"))?;
            let delta = Delta {
                data: data.clone(),
                base: None,
                key: k,
            };
            self.store.add(&delta, &Default::default())?;
        }

        Ok(())
    }
}

impl DataStore for FakeRemoteDataStore {
    fn get(&self, _key: &Key) -> Result<Option<Vec<u8>>> {
        unreachable!();
    }

    fn get_delta(&self, key: &Key) -> Result<Option<Delta>> {
        match self.prefetch(vec![key.clone()]) {
            Err(_) => Ok(None),
            Ok(()) => self.store.get_delta(key),
        }
    }

    fn get_delta_chain(&self, key: &Key) -> Result<Option<Vec<Delta>>> {
        match self.prefetch(vec![key.clone()]) {
            Err(_) => Ok(None),
            Ok(()) => self.store.get_delta_chain(key),
        }
    }

    fn get_meta(&self, key: &Key) -> Result<Option<Metadata>> {
        match self.prefetch(vec![key.clone()]) {
            Err(_) => Ok(None),
            Ok(()) => self.store.get_meta(key),
        }
    }
}

impl LocalStore for FakeRemoteDataStore {
    fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>> {
        Ok(keys.to_vec())
    }
}

struct FakeRemoteHistoryStore {
    store: Box<dyn MutableHistoryStore>,
    map: HashMap<Key, NodeInfo>,
}

impl RemoteHistoryStore for FakeRemoteHistoryStore {
    fn prefetch(&self, keys: Vec<Key>) -> Result<()> {
        for k in keys {
            self.store
                .add(&k, self.map.get(&k).ok_or(err_msg("Not found"))?)?
        }

        Ok(())
    }
}

impl HistoryStore for FakeRemoteHistoryStore {
    fn get_node_info(&self, key: &Key) -> Result<Option<NodeInfo>> {
        match self.prefetch(vec![key.clone()]) {
            Err(_) => Ok(None),
            Ok(()) => self.store.get_node_info(key),
        }
    }
}

impl LocalStore for FakeRemoteHistoryStore {
    fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>> {
        Ok(keys.to_vec())
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
                    .ok_or("Not found".into())
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

pub fn fake_edenapi(map: HashMap<Key, Bytes>) -> Box<dyn EdenApi> {
    Box::new(FakeEdenApi::new(map))
}
