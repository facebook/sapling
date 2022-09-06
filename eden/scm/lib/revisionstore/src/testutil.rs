/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use edenapi::EdenApi;
use edenapi::EdenApiError;
use edenapi::Response;
use edenapi::ResponseMeta;
use edenapi::Stats;
use edenapi_types::EdenApiServerError;
use edenapi_types::FileAttributes;
use edenapi_types::FileContent;
use edenapi_types::FileEntry;
use edenapi_types::FileResponse;
use edenapi_types::FileSpec;
use edenapi_types::HistoryEntry;
use edenapi_types::TreeAttributes;
use edenapi_types::TreeEntry;
use futures::prelude::*;
#[cfg(test)]
pub use lfs_mocks::*;
use minibytes::Bytes;
use types::Key;
use types::NodeInfo;
use types::Parents;

use crate::datastore::Delta;
use crate::datastore::HgIdDataStore;
use crate::datastore::HgIdMutableDeltaStore;
use crate::datastore::Metadata;
use crate::datastore::RemoteDataStore;
use crate::datastore::StoreResult;
use crate::historystore::HgIdHistoryStore;
use crate::historystore::HgIdMutableHistoryStore;
use crate::historystore::RemoteHistoryStore;
use crate::localstore::LocalStore;
use crate::remotestore::HgIdRemoteStore;
use crate::scmstore::file::LazyFile;
use crate::types::StoreKey;

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
    fn prefetch(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
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

        self.store.get_missing(keys)
    }

    fn upload(&self, _keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        unimplemented!()
    }
}

impl HgIdDataStore for FakeRemoteDataStore {
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        match self.prefetch(&[key.clone()]) {
            Err(_) => Ok(StoreResult::NotFound(key)),
            Ok(_) => self.store.get(key),
        }
    }

    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>> {
        match self.prefetch(&[key.clone()]) {
            Err(_) => Ok(StoreResult::NotFound(key)),
            Ok(_) => self.store.get_meta(key),
        }
    }

    fn refresh(&self) -> Result<()> {
        Ok(())
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

    fn refresh(&self) -> Result<()> {
        Ok(())
    }
}

impl LocalStore for FakeRemoteHistoryStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.store.get_missing(keys)
    }
}

#[derive(Default)]
pub struct FakeEdenApi {
    files: HashMap<Key, (Bytes, Option<u64>)>,
    trees: HashMap<Key, Bytes>,
    history: HashMap<Key, NodeInfo>,
}

impl FakeEdenApi {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn files(self, files: impl IntoIterator<Item = (Key, Bytes)>) -> Self {
        Self {
            files: files
                .into_iter()
                .map(|(key, bytes)| (key, (bytes, None)))
                .collect(),
            ..self
        }
    }

    /// See revisionstore::types::datastore::Metadata for how to construct these flags.
    ///
    /// Hint: None, or Some(Metadata::LFS_FLAG) are all you'll ever need.
    pub fn files_with_flags(self, files: HashMap<Key, (Bytes, Option<u64>)>) -> Self {
        Self { files, ..self }
    }

    pub fn trees(self, trees: HashMap<Key, Bytes>) -> Self {
        Self { trees, ..self }
    }

    pub fn history(self, history: HashMap<Key, NodeInfo>) -> Self {
        Self { history, ..self }
    }

    pub fn into_arc(self) -> Arc<Self> {
        Arc::new(self)
    }

    fn get_files(
        map: &HashMap<Key, (Bytes, Option<u64>)>,
        reqs: impl Iterator<Item = FileSpec>,
    ) -> Result<Response<FileResponse>, EdenApiError> {
        let entries = reqs
            .filter_map(|spec| {
                let parents = Parents::default();
                let mut entry = FileEntry::new(spec.key.clone(), parents);

                let (data, flags) = map.get(&spec.key)?.clone();
                let metadata = Metadata {
                    flags,
                    size: Some(data.len() as u64),
                };
                let data = data.to_vec().into();
                let content = FileContent {
                    hg_file_blob: data,
                    metadata,
                };

                if spec.attrs.aux_data {
                    // TODO(meyer): Compute aux data directly.
                    let mut file = LazyFile::EdenApi(entry.clone().with_content(content.clone()));
                    let aux = file.aux_data().ok()?;
                    entry = entry.with_aux_data(aux.into());
                }

                if spec.attrs.content {
                    entry = entry.with_content(content);
                }

                Some(Ok(FileResponse {
                    key: spec.key,
                    result: Ok(entry),
                }))
            })
            .collect::<Vec<_>>();

        Ok(Response {
            entries: Box::pin(stream::iter(entries)),
            stats: Box::pin(future::ok(Stats::default())),
        })
    }

    fn get_trees(
        map: &HashMap<Key, Bytes>,
        keys: Vec<Key>,
    ) -> Result<Response<Result<TreeEntry, EdenApiServerError>>, EdenApiError> {
        let entries = keys
            .into_iter()
            .filter_map(|key| {
                let data = map.get(&key)?.clone();
                let parents = Parents::default();
                let data = data.to_vec().into();
                Some(Ok(Ok(TreeEntry::new(key, data, parents))))
            })
            .collect::<Vec<_>>();

        Ok(Response {
            entries: Box::pin(stream::iter(entries)),
            stats: Box::pin(future::ok(Stats::default())),
        })
    }
}

#[async_trait]
impl EdenApi for FakeEdenApi {
    async fn health(&self) -> Result<ResponseMeta, EdenApiError> {
        Ok(ResponseMeta::default())
    }

    async fn files(&self, keys: Vec<Key>) -> Result<Response<FileResponse>, EdenApiError> {
        Self::get_files(
            &self.files,
            keys.into_iter().map(|key| FileSpec {
                key,
                attrs: FileAttributes {
                    content: true,
                    aux_data: false,
                },
            }),
        )
    }

    async fn files_attrs(
        &self,
        reqs: Vec<FileSpec>,
    ) -> Result<Response<FileResponse>, EdenApiError> {
        Self::get_files(&self.files, reqs.into_iter())
    }

    async fn history(
        &self,
        keys: Vec<Key>,
        _length: Option<u32>,
    ) -> Result<Response<HistoryEntry>, EdenApiError> {
        let entries = keys
            .into_iter()
            .filter_map(|key| {
                let nodeinfo = self.history.get(&key)?.clone();
                Some(Ok(HistoryEntry { key, nodeinfo }))
            })
            .collect::<Vec<_>>();

        Ok(Response {
            entries: Box::pin(stream::iter(entries)),
            stats: Box::pin(future::ok(Stats::default())),
        })
    }

    async fn trees(
        &self,
        keys: Vec<Key>,
        _attrs: Option<TreeAttributes>,
    ) -> Result<Response<Result<TreeEntry, EdenApiServerError>>, EdenApiError> {
        Self::get_trees(&self.trees, keys)
    }
}

pub fn make_config(dir: impl AsRef<Path>) -> BTreeMap<String, String> {
    [
        ("remotefilelog.reponame", "test".to_string()),
        (
            "remotefilelog.cachepath",
            dir.as_ref().display().to_string(),
        ),
        (
            "remotefilelog.cachekey",
            "cca::hg:rust_unittest".to_string(),
        ),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.clone()))
    .collect()
}

pub(crate) fn empty_config() -> BTreeMap<String, String> {
    BTreeMap::new()
}

#[cfg(test)]
mod lfs_mocks {
    use lfs_protocol::ObjectAction;
    use lfs_protocol::ObjectError;
    use lfs_protocol::ObjectStatus;
    use lfs_protocol::Operation;
    use lfs_protocol::RequestObject;
    use lfs_protocol::ResponseBatch;
    use lfs_protocol::ResponseObject;
    use lfs_protocol::Sha256 as LfsSha256;
    use lfs_protocol::Transfer;
    use mockito::mock;
    use mockito::Mock;
    use types::Sha256;

    use super::*;

    pub struct TestBlob {
        pub oid: &'static str,
        pub size: usize,
        pub content: Bytes,
        pub sha: Sha256,
        pub response: Vec<&'static [u8]>,
        pub error: bool,
        pub chunk_size: Option<usize>,
    }

    pub fn example_blob() -> TestBlob {
        use std::str::FromStr;

        let blob_oid = "fc613b4dfd6736a7bd268c8a0e74ed0d1c04a959f59dd74ef2874983fd443fc9";
        let content = b"master";

        TestBlob {
            oid: blob_oid,
            size: 6,
            content: Bytes::from(&content[..]),
            sha: Sha256::from_str(blob_oid).unwrap(),
            response: vec![content],
            error: false,
            chunk_size: None,
        }
    }

    pub fn example_blob2() -> TestBlob {
        use std::str::FromStr;
        let blob2_oid = "ca3e228a1d8d845064112c4e92781f6b8fc2501f0aa0e415d4a1dcc941485b24";
        let content = b"1.44.0";
        TestBlob {
            oid: blob2_oid,
            size: 6,
            content: Bytes::from(&content[..]),
            sha: Sha256::from_str(blob2_oid).unwrap(),
            response: vec![content],
            error: false,
            chunk_size: None,
        }
    }

    pub fn nonexistent_blob() -> TestBlob {
        use std::str::FromStr;
        let blob3_oid = "0000000000000000000000000000000000000000000000000000000000000000";
        TestBlob {
            oid: blob3_oid,
            size: 0,
            content: Bytes::from(&b""[..]),
            sha: Sha256::from_str(blob3_oid).unwrap(),
            response: vec![b"not_reached"],
            error: true,
            chunk_size: None,
        }
    }

    pub fn get_lfs_batch_mock(status: usize, blobs: &[&TestBlob]) -> Mock {
        let objects = blobs
            .iter()
            .map(|tb| {
                let object = RequestObject {
                    oid: LfsSha256(tb.sha.into_inner()),
                    size: tb.size as u64,
                };

                let status = if tb.error {
                    ObjectStatus::Err {
                        error: ObjectError {
                            code: 404,
                            message: "".into(),
                        },
                    }
                } else {
                    ObjectStatus::Ok {
                        authenticated: false,
                        actions: vec![(
                            Operation::Download,
                            ObjectAction {
                                href: format!("{}/repo/download/{}", mockito::server_url(), tb.oid)
                                    .as_str()
                                    .try_into()
                                    .unwrap(),
                                expires_at: None,
                                expires_in: None,
                                header: None,
                            },
                        )]
                        .into_iter()
                        .collect(),
                    }
                };

                ResponseObject { object, status }
            })
            .collect();

        let r = ResponseBatch {
            transfer: Transfer::Basic,
            objects,
        };

        let json_response = serde_json::to_string(&r).unwrap();

        mock("POST", "/repo/objects/batch")
            .with_status(status)
            .with_body(json_response)
            .create()
    }

    pub fn get_lfs_download_mock(status: usize, blob: &TestBlob) -> Vec<Mock> {
        let mut mocks = vec![];
        for response in blob.response.iter() {
            let m = mock("GET", format!("/repo/download/{}", blob.oid).as_str())
                .with_status(status)
                .with_body(response)
                .with_header("content-type", "application/octet-stream");

            mocks.push(m);
        }

        let mocks = if let Some(chunk_size) = blob.chunk_size {
            let mut i = 0;
            let mut chunked_mocks: Vec<Mock> = vec![];

            for mock in mocks.into_iter() {
                let m = mock.with_status(206).match_header(
                    "Range",
                    format!("bytes={}-{}", i, i + chunk_size - 1).as_str(),
                );
                chunked_mocks.push(m);
                i += chunk_size;
            }

            chunked_mocks
        } else {
            mocks
        };

        mocks.into_iter().map(|m| m.create()).collect()
    }

    pub fn make_lfs_config(dir: impl AsRef<Path>, agent_sufix: &str) -> BTreeMap<String, String> {
        let mut config = make_config(dir);
        let mut set = |key: &str, value: &str| {
            config.insert(key.to_string(), value.to_string());
        };
        set(
            "lfs.url",
            &[mockito::server_url(), "/repo".to_string()].concat(),
        );
        set("lfs.use-client-certs", "false");
        set(
            "experimental.lfs.user-agent",
            &format!("mercurial/revisionstore/unittests/{}", agent_sufix),
        );
        set("lfs.threshold", "4");
        set("remotefilelog.lfs", "true");
        set("lfs.moveafterupload", "true");

        config
    }
}

pub(crate) fn setconfig(
    config: &mut BTreeMap<String, String>,
    section: &str,
    name: &str,
    value: &str,
) {
    config.insert(format!("{}.{}", section, name), value.to_string());
}
