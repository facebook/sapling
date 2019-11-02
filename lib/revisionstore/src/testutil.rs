/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{collections::HashMap, time::Duration};

use bytes::Bytes;

use edenapi::{ApiResult, DownloadStats, EdenApi, ProgressFn};
use types::{HgId, HistoryEntry, Key, RepoPathBuf};

use crate::datastore::Delta;

pub fn delta(data: &str, base: Option<Key>, key: Key) -> Delta {
    Delta {
        data: Bytes::from(data),
        base,
        key,
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
