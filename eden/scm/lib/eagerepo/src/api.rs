/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::EagerRepo;
use dag::ops::DagExportCloneData;
use dag::protocol::AncestorPath;
use dag::protocol::RemoteIdConvertProtocol;
use dag::Location;
use dag::Vertex;
use edenapi::types::BookmarkEntry;
use edenapi::types::CommitHashToLocationResponse;
use edenapi::types::CommitKnownResponse;
use edenapi::types::CommitLocationToHashRequest;
use edenapi::types::CommitLocationToHashResponse;
use edenapi::types::CommitRevlogData;
use edenapi::types::FileEntry;
use edenapi::types::HgId;
use edenapi::types::HistoryEntry;
use edenapi::types::Key;
use edenapi::types::NodeInfo;
use edenapi::types::Parents;
use edenapi::types::RepoPathBuf;
use edenapi::types::TreeAttributes;
use edenapi::types::TreeEntry;
use edenapi::EdenApi;
use edenapi::EdenApiError;
use edenapi::Fetch;
use edenapi::ProgressCallback;
use edenapi::ResponseMeta;
use http::StatusCode;
use http::Version;
use std::collections::HashSet;

#[async_trait::async_trait]
impl EdenApi for EagerRepo {
    async fn health(&self) -> edenapi::Result<ResponseMeta> {
        Ok(default_response_meta())
    }

    async fn files(
        &self,
        _repo: String,
        keys: Vec<Key>,
        _progress: Option<ProgressCallback>,
    ) -> edenapi::Result<Fetch<FileEntry>> {
        let mut values = Vec::with_capacity(keys.len());
        for key in keys {
            let id = key.hgid;
            let data = self.get_sha1_blob_for_api(id)?;
            let (p1, p2) = extract_p1_p2(&data);
            let parents = Parents::new(p1, p2);
            let entry = FileEntry {
                key,
                parents,
                // PERF: to_vec().into() converts minibytes::Bytes to bytes::Bytes.
                data: extract_body(&data).to_vec().into(),
                metadata: Default::default(),
            };
            values.push(Ok(entry));
        }
        Ok(convert_to_fetch(values))
    }

    async fn history(
        &self,
        _repo: String,
        keys: Vec<Key>,
        _length: Option<u32>,
        _progress: Option<ProgressCallback>,
    ) -> edenapi::Result<Fetch<HistoryEntry>> {
        let mut values = Vec::new();
        let mut visited: HashSet<Key> = Default::default();
        let mut to_visit: Vec<Key> = keys;
        while let Some(key) = to_visit.pop() {
            if !visited.insert(key.clone()) {
                continue;
            }
            let data = self.get_sha1_blob_for_api(key.hgid)?;
            // NOTE: Order of p1, p2 are not preserved, unlike revlog hg.
            // It should be okay correctness-wise.
            let (p1, p2) = extract_p1_p2(&data);
            let mut key1 = Key {
                path: key.path.clone(),
                hgid: p1,
            };
            let mut key2 = Key {
                path: key.path.clone(),
                hgid: p2,
            };
            if let Some(renamed_from) = extract_rename(extract_body(&data)) {
                if p1.is_null() {
                    key1 = renamed_from;
                } else {
                    key2 = renamed_from;
                }
            }
            if !p1.is_null() {
                to_visit.push(key1.clone());
            }
            if !p2.is_null() {
                to_visit.push(key2.clone());
            }
            let entry = HistoryEntry {
                key,
                nodeinfo: NodeInfo {
                    parents: [key1, key2],
                    linknode: *HgId::null_id(),
                },
            };
            values.push(Ok(entry));
        }
        Ok(convert_to_fetch(values))
    }

    async fn trees(
        &self,
        _repo: String,
        keys: Vec<Key>,
        attributes: Option<TreeAttributes>,
        _progress: Option<ProgressCallback>,
    ) -> edenapi::Result<Fetch<Result<TreeEntry, edenapi::types::EdenApiServerError>>> {
        let mut values = Vec::new();
        let attributes = attributes.unwrap_or_default();
        if attributes.child_metadata {
            return Err(not_implemented_error(
                "EagerRepo does not support child_metadata for trees".to_string(),
            ));
        }
        for key in keys {
            let data = self.get_sha1_blob_for_api(key.hgid)?;
            let mut entry = TreeEntry::default();
            entry.key = key;
            if attributes.manifest_blob {
                // PERF: to_vec().into() converts minibytes::Bytes to bytes::Bytes.
                entry.data = Some(extract_body(&data).to_vec().into());
            }
            if attributes.parents {
                let (p1, p2) = extract_p1_p2(&data);
                let parents = Parents::new(p1, p2);
                entry.parents = Some(parents);
            }
            assert!(!attributes.child_metadata, "checked above");
            values.push(Ok(Ok(entry)));
        }
        Ok(convert_to_fetch(values))
    }

    async fn complete_trees(
        &self,
        _repo: String,
        _rootdir: RepoPathBuf,
        _mfnodes: Vec<HgId>,
        _basemfnodes: Vec<HgId>,
        _depth: Option<usize>,
        _progress: Option<ProgressCallback>,
    ) -> edenapi::Result<Fetch<Result<TreeEntry, edenapi::types::EdenApiServerError>>> {
        Err(not_implemented_error(
            "EagerRepo does not support complete_trees endpoint".to_string(),
        ))
    }

    async fn commit_revlog_data(
        &self,
        _repo: String,
        hgids: Vec<HgId>,
        _progress: Option<ProgressCallback>,
    ) -> edenapi::Result<Fetch<CommitRevlogData>> {
        let mut values = Vec::new();
        for id in hgids {
            let data = self.get_sha1_blob_for_api(id)?;
            let data = CommitRevlogData {
                hgid: id,
                // PERF: to_vec().into() converts minibytes::Bytes to bytes::Bytes.
                revlog_data: data.to_vec().into(),
            };
            values.push(Ok(data));
        }
        Ok(convert_to_fetch(values))
    }

    async fn clone_data(
        &self,
        _repo: String,
        _progress: Option<ProgressCallback>,
    ) -> edenapi::Result<dag::CloneData<HgId>> {
        let clone_data = self.dag().export_clone_data().await.map_err(map_dag_err)?;
        check_convert_to_hgid(clone_data.idmap.values())?;
        let clone_data = dag::CloneData {
            flat_segments: clone_data.flat_segments,
            idmap: clone_data
                .idmap
                .into_iter()
                .map(|(k, v)| (k, HgId::from_slice(v.as_ref()).unwrap())) // unwrap: checked above
                .collect(),
        };
        Ok(clone_data)
    }

    async fn full_idmap_clone_data(
        &self,
        _repo: String,
        _progress: Option<ProgressCallback>,
    ) -> edenapi::Result<dag::CloneData<HgId>> {
        Err(not_implemented_error(
            "EagerRepo does not support full_idmap_clone_data endpoint".to_string(),
        ))
    }

    async fn commit_location_to_hash(
        &self,
        _repo: String,
        requests: Vec<CommitLocationToHashRequest>,
        _progress: Option<ProgressCallback>,
    ) -> edenapi::Result<Fetch<CommitLocationToHashResponse>> {
        let path_names: Vec<(AncestorPath, Vec<Vertex>)> = {
            let paths: Vec<AncestorPath> = requests
                .into_iter()
                .map(|r| AncestorPath {
                    x: Vertex::copy_from(r.location.descendant.as_ref()),
                    n: r.location.distance,
                    batch_size: r.count,
                })
                .collect();
            self.dag()
                .resolve_relative_paths_to_names(paths)
                .await
                .map_err(map_dag_err)?
        };

        check_convert_to_hgid(path_names.iter().flat_map(|i| i.1.iter()))?;
        check_convert_to_hgid(path_names.iter().map(|i| &i.0.x))?;

        let values: Vec<edenapi::Result<CommitLocationToHashResponse>> = path_names
            .into_iter()
            .map(|(p, ns)| {
                let count = ns.len();
                let response = CommitLocationToHashResponse {
                    location: Location {
                        descendant: HgId::from_slice(p.x.as_ref()).unwrap(), // unwrap: checked above
                        distance: p.n,
                    },
                    hgids: ns
                        .into_iter()
                        .map(|n| HgId::from_slice(n.as_ref()).unwrap()) // unwrap: checked above
                        .collect(),
                    count: count as _,
                };
                Ok(response)
            })
            .collect();

        Ok(convert_to_fetch(values))
    }

    async fn commit_hash_to_location(
        &self,
        _repo: String,
        master_heads: Vec<HgId>,
        hgids: Vec<HgId>,
        _progress: Option<ProgressCallback>,
    ) -> edenapi::Result<Fetch<CommitHashToLocationResponse>> {
        let path_names: Vec<(AncestorPath, Vec<Vertex>)> = {
            let heads: Vec<Vertex> = master_heads
                .into_iter()
                .map(|i| Vertex::copy_from(i.as_ref()))
                .collect();
            let names: Vec<Vertex> = hgids
                .into_iter()
                .map(|i| Vertex::copy_from(i.as_ref()))
                .collect();
            self.dag()
                .resolve_names_to_relative_paths(heads, names)
                .await
                .map_err(map_dag_err)?
        };

        check_convert_to_hgid(path_names.iter().flat_map(|i| i.1.iter()))?;
        check_convert_to_hgid(path_names.iter().map(|i| &i.0.x))?;

        let values: Vec<edenapi::Result<CommitHashToLocationResponse>> = path_names
            .into_iter()
            .flat_map(|(p, ns)| {
                ns.into_iter()
                    .enumerate()
                    .map(|(i, n)| {
                        CommitHashToLocationResponse {
                            hgid: HgId::from_slice(n.as_ref()).unwrap(), // unwrap: checked above
                            result: Ok(Some(Location {
                                descendant: HgId::from_slice(p.x.as_ref()).unwrap(), // unwrap: checked above
                                distance: p.n + (i as u64),
                            })),
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .map(Ok)
            .collect();

        // For hgids outside the master group, just ignore them.
        // It's okay to return them with result "None" too.

        Ok(convert_to_fetch(values))
    }

    async fn commit_known(
        &self,
        _repo: String,
        hgids: Vec<HgId>,
    ) -> edenapi::Result<Fetch<CommitKnownResponse>> {
        todo!()
    }

    async fn bookmarks(
        &self,
        _repo: String,
        bookmarks: Vec<String>,
        _progress: Option<ProgressCallback>,
    ) -> edenapi::Result<Fetch<BookmarkEntry>> {
        let mut values = Vec::new();
        let map = self.get_bookmarks_map().map_err(map_crate_err)?;
        for name in bookmarks {
            let opt_id = map.get(&name).cloned();
            let entry = BookmarkEntry {
                bookmark: name,
                hgid: opt_id,
            };
            values.push(Ok(entry));
        }
        Ok(convert_to_fetch(values))
    }
}

impl EagerRepo {
    fn get_sha1_blob_for_api(&self, id: HgId) -> edenapi::Result<minibytes::Bytes> {
        // Emulate the HTTP errors.
        match self.get_sha1_blob(id) {
            Ok(None) => Err(EdenApiError::HttpError {
                status: StatusCode::NOT_FOUND,
                message: format!("{} cannot be found", id.to_hex()),
            }),
            Ok(Some(data)) => Ok(data),
            Err(e) => Err(EdenApiError::HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!("{:?}", e),
            }),
        }
    }
}

fn default_response_meta() -> ResponseMeta {
    ResponseMeta {
        version: Version::HTTP_11,
        status: StatusCode::OK,
        server: Some("EagerRepo".to_string()),
        ..Default::default()
    }
}

fn extract_body(data_with_p1p2_prefix: &[u8]) -> &[u8] {
    &data_with_p1p2_prefix[HgId::len() * 2..]
}

fn extract_p1_p2(data: &[u8]) -> (HgId, HgId) {
    let p2 = HgId::from_slice(&data[..HgId::len()]).unwrap();
    let p1 = HgId::from_slice(&data[HgId::len()..(HgId::len() * 2)]).unwrap();
    (p1, p2)
}

/// Extract rename metadata from filelog header (if rename exists).
/// data is not prefixed by hashes.
///
/// See `filelog.py:parsemeta`.
fn extract_rename(data: &[u8]) -> Option<Key> {
    if data.starts_with(b"\x01\n") {
        let data = &data[2..];
        if let Some(pos) = data.windows(2).position(|needle| needle == b"\x01\n") {
            let header = String::from_utf8_lossy(&data[..pos]);
            let mut path = None;
            let mut rev = None;
            for line in header.lines() {
                let kv: Vec<&str> = line.split(": ").collect();
                if let [k, v] = &kv[..] {
                    if *k == "copy" {
                        path = RepoPathBuf::from_string(v.to_string()).ok();
                    } else if *k == "copyrev" {
                        rev = HgId::from_hex(v.as_bytes()).ok();
                    }
                }
            }
            if let (Some(path), Some(rev)) = (path, rev) {
                return Some(Key {
                    path: path.into(),
                    hgid: rev,
                });
            }
        }
    }
    None
}

/// Convert `Vec<T>` to `Fetch<T>`.
fn convert_to_fetch<T: Send + Sync + 'static>(values: Vec<edenapi::Result<T>>) -> Fetch<T> {
    Fetch {
        meta: Default::default(),
        stats: Box::pin(async { Ok(Default::default()) }),
        entries: Box::pin(futures::stream::iter(values)),
    }
}

/// Not implement error.
fn not_implemented_error(message: String) -> EdenApiError {
    EdenApiError::HttpError {
        status: StatusCode::NOT_IMPLEMENTED,
        message,
    }
}

fn check_convert_to_hgid<'a>(vertexes: impl Iterator<Item = &'a Vertex>) -> edenapi::Result<()> {
    for v in vertexes {
        let _ = HgId::from_slice(v.as_ref()).map_err(|e| EdenApiError::Other(e.into()))?;
    }
    Ok(())
}

fn map_dag_err(e: dag::Error) -> EdenApiError {
    EdenApiError::Other(e.into())
}

fn map_crate_err(e: crate::Error) -> EdenApiError {
    EdenApiError::Other(e.into())
}
