/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::sync::Arc;

use anyhow::bail;
use anyhow::format_err;
use anyhow::Context;
use async_runtime::block_unless_interrupted;
use cpython::*;
use cpython_async::PyFuture;
use cpython_async::TStream;
use cpython_ext::convert::Serde;
use cpython_ext::PyCell;
use cpython_ext::PyPathBuf;
use cpython_ext::ResultPyErrExt;
use dag_types::Location;
use dag_types::VertexName;
use edenapi::EdenApi;
use edenapi::EdenApiError;
use edenapi::Response;
use edenapi::Stats;
use edenapi_ext::calc_contentid;
use edenapi_ext::check_files;
use edenapi_ext::download_files;
use edenapi_types::AnyFileContentId;
use edenapi_types::AnyId;
use edenapi_types::CommitGraphEntry;
use edenapi_types::CommitHashLookupResponse;
use edenapi_types::CommitHashToLocationResponse;
use edenapi_types::CommitKnownResponse;
use edenapi_types::CommitLocationToHashRequest;
use edenapi_types::CommitLocationToHashResponse;
use edenapi_types::CommitRevlogData;
use edenapi_types::EdenApiServerError;
use edenapi_types::FetchSnapshotRequest;
use edenapi_types::FetchSnapshotResponse;
use edenapi_types::FileEntry;
use edenapi_types::HgChangesetContent;
use edenapi_types::HgFilenodeData;
use edenapi_types::HgMutationEntryContent;
use edenapi_types::HistoryEntry;
use edenapi_types::IndexableId;
use edenapi_types::LandStackResponse;
use edenapi_types::LookupResult;
use edenapi_types::TreeAttributes;
use edenapi_types::TreeEntry;
use edenapi_types::UploadHgChangeset;
use edenapi_types::UploadToken;
use futures::prelude::*;
use futures::stream;
use progress_model::ProgressBar;
use pyrevisionstore::as_legacystore;
use revisionstore::datastore::separate_metadata;
use revisionstore::HgIdMutableDeltaStore;
use revisionstore::StoreKey;
use revisionstore::StoreResult;
use types::HgId;
use types::RepoPathBuf;

use crate::pytypes::PyStats;
use crate::stats::stats;
use crate::util::as_deltastore;
use crate::util::meta_to_dict;
use crate::util::to_contentid;
use crate::util::to_keys;
use crate::util::to_keys_with_parents;
use crate::util::to_path;
use crate::util::to_trees_upload_items;

/// Extension trait allowing EdenAPI methods to be called from Python code.
///
/// One nice benefit of making this a trait instead of directly implementing
/// the methods inside a `py_class!` macro invocation is that tools like
/// `rustfmt` can still parse the code.
pub trait EdenApiPyExt: EdenApi {
    fn health_py(&self, py: Python) -> PyResult<PyDict> {
        let meta = block_unless_interrupted(self.health())
            .map_pyerr(py)?
            .map_pyerr(py)?;
        meta_to_dict(py, &meta)
    }

    fn files_py(
        &self,
        py: Python,
        repo: String,
        keys: Vec<(PyPathBuf, Serde<HgId>)>,
    ) -> PyResult<TStream<anyhow::Result<Serde<FileEntry>>>> {
        let keys = to_keys(py, &keys)?;
        let entries = py
            .allow_threads(|| block_unless_interrupted(self.files(repo, keys)))
            .map_pyerr(py)?
            .map_pyerr(py)?
            .entries;
        Ok(entries.map_ok(Serde).map_err(Into::into).into())
    }

    fn history_py(
        &self,
        py: Python,
        repo: String,
        keys: Vec<(PyPathBuf, Serde<HgId>)>,
        length: Option<u32>,
    ) -> PyResult<TStream<anyhow::Result<Serde<HistoryEntry>>>> {
        let keys = to_keys(py, &keys)?;
        let entries = py
            .allow_threads(|| block_unless_interrupted(self.history(repo, keys, length)))
            .map_pyerr(py)?
            .map_pyerr(py)?
            .entries;
        Ok(entries.map_ok(Serde).map_err(Into::into).into())
    }

    fn storetrees_py(
        &self,
        py: Python,
        store: PyObject,
        repo: String,
        keys: Vec<(PyPathBuf, Serde<HgId>)>,
        attributes: Option<TreeAttributes>,
    ) -> PyResult<stats> {
        let keys = to_keys(py, &keys)?;
        let store = as_deltastore(py, store)?;

        let stats = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let prog = ProgressBar::register_new(
                        "Downloading trees over HTTP",
                        keys.len() as u64,
                        "trees",
                    );
                    let response = self.trees(repo, keys, attributes).await?;
                    write_trees(response, store, prog).await
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        stats::new(py, stats)
    }

    fn trees_py(
        &self,
        py: Python,
        repo: String,
        keys: Vec<(PyPathBuf, Serde<HgId>)>,
        attributes: Option<TreeAttributes>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<TreeEntry>>>, PyFuture)> {
        let keys = to_keys(py, &keys)?;
        let (trees, stats) = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let response = self.trees(repo, keys, attributes).await?;
                    Ok::<_, EdenApiError>((response.entries, response.stats))
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        let trees_py = trees.map(|t| match t {
            Ok(Ok(t)) => Ok(Serde(t)),
            Ok(Err(e)) => Err(e.into()),
            Err(e) => Err(e.into()),
        });
        let stats_py = PyFuture::new(py, stats.map_ok(PyStats))?;
        Ok((trees_py.into(), stats_py))
    }

    fn commit_revlog_data_py(
        &self,
        py: Python,
        repo: String,
        nodes: Vec<HgId>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<CommitRevlogData>>>, PyFuture)> {
        let (commits, stats) = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let response = self.commit_revlog_data(repo, nodes).await?;
                    let commits = response.entries;
                    let stats = response.stats;
                    Ok::<_, EdenApiError>((commits, stats))
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        let commits_py = commits.map_ok(Serde).map_err(Into::into);
        let stats_py = PyFuture::new(py, stats.map_ok(PyStats))?;
        Ok((commits_py.into(), stats_py))
    }

    fn bookmarks_py(&self, py: Python, repo: String, bookmarks: Vec<String>) -> PyResult<PyDict> {
        let response = py
            .allow_threads(|| block_unless_interrupted(self.bookmarks(repo, bookmarks)))
            .map_pyerr(py)?
            .map_pyerr(py)?;

        let bookmarks = PyDict::new(py);
        for entry in response {
            bookmarks.set_item(py, entry.bookmark, entry.hgid.map(|id| id.to_hex()))?;
        }
        Ok(bookmarks)
    }

    fn set_bookmark_py(
        &self,
        py: Python,
        repo: String,
        bookmark: String,
        to: Option<HgId>,
        from: Option<HgId>,
        pushvars: Vec<(String, String)>,
    ) -> PyResult<bool> {
        py.allow_threads(|| {
            block_unless_interrupted(async move {
                self.set_bookmark(repo, bookmark, to, from, pushvars.into_iter().collect())
                    .await?;
                Ok::<(), EdenApiError>(())
            })
        })
        .map_pyerr(py)?
        .map_pyerr(py)?;

        Ok(true)
    }

    fn land_stack_py(
        &self,
        py: Python,
        repo: String,
        bookmark: String,
        head: HgId,
        base: HgId,
        pushvars: Vec<(String, String)>,
    ) -> PyResult<Serde<LandStackResponse>> {
        let response = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let response = self
                        .land_stack(repo, bookmark, head, base, pushvars.into_iter().collect())
                        .await?;
                    Ok::<_, EdenApiError>(response)
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        Ok(Serde(response))
    }

    fn hash_lookup_py(
        &self,
        py: Python,
        repo: String,
        hash_prefixes: Vec<String>,
    ) -> PyResult<Serde<Vec<CommitHashLookupResponse>>> {
        let items = py
            .allow_threads(|| {
                block_unless_interrupted(self.hash_prefixes_lookup(repo, hash_prefixes))
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;
        Ok(Serde(items))
    }

    fn commit_location_to_hash_py(
        &self,
        py: Python,
        repo: String,
        requests: Vec<(HgId, u64, u64)>,
    ) -> PyResult<Serde<Vec<CommitLocationToHashResponse>>> {
        let requests = requests
            .into_iter()
            .map(|(hgid, distance, count)| {
                let location = Location::new(hgid, distance);
                CommitLocationToHashRequest { location, count }
            })
            .collect();
        let responses = py
            .allow_threads(|| {
                block_unless_interrupted(self.commit_location_to_hash(repo, requests))
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        Ok(Serde(responses))
    }

    fn commit_hash_to_location_py(
        &self,
        py: Python,
        repo: String,
        master_heads: Vec<HgId>,
        hgids: Vec<HgId>,
    ) -> PyResult<Serde<Vec<CommitHashToLocationResponse>>> {
        let responses = py
            .allow_threads(|| {
                block_unless_interrupted(self.commit_hash_to_location(repo, master_heads, hgids))
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        Ok(Serde(responses))
    }

    fn commit_known_py(
        &self,
        py: Python,
        repo: String,
        hgids: Vec<HgId>,
    ) -> PyResult<Serde<Vec<CommitKnownResponse>>> {
        let responses = py
            .allow_threads(|| block_unless_interrupted(self.commit_known(repo, hgids)))
            .map_pyerr(py)?
            .map_pyerr(py)?;
        Ok(Serde(responses))
    }

    fn commit_graph_py(
        &self,
        py: Python,
        repo: String,
        heads: Vec<HgId>,
        common: Vec<HgId>,
    ) -> PyResult<Serde<Vec<CommitGraphEntry>>> {
        let responses = py
            .allow_threads(|| block_unless_interrupted(self.commit_graph(repo, heads, common)))
            .map_pyerr(py)?
            .map_pyerr(py)?;

        Ok(Serde(responses))
    }

    /// Get the "CloneData" serialized using mincode.
    fn clone_data_py(&self, py: Python, repo: String) -> PyResult<PyCell> {
        let data = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    self.clone_data(repo).await.map(|data| {
                        data.convert_vertex(|hgid| VertexName(hgid.as_ref().to_vec().into()))
                    })
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;
        PyCell::new(py, data)
    }

    /// Get pll data for master pull fast path
    fn pull_fast_forward_master_py(
        &self,
        py: Python,
        repo: String,
        old_master: HgId,
        new_master: HgId,
    ) -> PyResult<PyCell> {
        let data = {
            py.allow_threads(|| {
                block_unless_interrupted(async move {
                    match self
                        .pull_fast_forward_master(repo, old_master, new_master)
                        .await
                    {
                        Err(e) => Err(e),
                        Ok(data) => Ok(data.convert_vertex(|hgid| {
                            VertexName(hgid.into_byte_array().to_vec().into())
                        })),
                    }
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?
        };
        PyCell::new(py, data)
    }

    fn lookup_file_contents(
        &self,
        py: Python,
        repo: String,
        ids: Vec<PyBytes>,
    ) -> PyResult<Serde<Vec<(usize, UploadToken)>>> {
        self.lookup_py(
            py,
            repo,
            ids.into_iter()
                .map(|id| {
                    AnyId::AnyFileContentId(AnyFileContentId::ContentId(to_contentid(py, &id)))
                })
                .collect(),
        )
    }

    fn lookup_commits(
        &self,
        py: Python,
        repo: String,
        nodes: Vec<HgId>,
    ) -> PyResult<Serde<Vec<(usize, UploadToken)>>> {
        self.lookup_py(
            py,
            repo,
            nodes.into_iter().map(AnyId::HgChangesetId).collect(),
        )
    }

    fn lookup_filenodes(
        &self,
        py: Python,
        repo: String,
        ids: Vec<HgId>,
    ) -> PyResult<Serde<Vec<(usize, UploadToken)>>> {
        self.lookup_py(py, repo, ids.into_iter().map(AnyId::HgFilenodeId).collect())
    }

    fn lookup_trees(
        &self,
        py: Python,
        repo: String,
        ids: Vec<HgId>,
    ) -> PyResult<Serde<Vec<(usize, UploadToken)>>> {
        self.lookup_py(py, repo, ids.into_iter().map(AnyId::HgTreeId).collect())
    }

    fn lookup_filenodes_and_trees(
        &self,
        py: Python,
        repo: String,
        filenodes_ids: Vec<HgId>,
        trees_ids: Vec<HgId>,
    ) -> PyResult<Serde<Vec<(usize, UploadToken)>>> {
        self.lookup_py(
            py,
            repo,
            filenodes_ids
                .into_iter()
                .map(AnyId::HgFilenodeId)
                .chain(trees_ids.into_iter().map(AnyId::HgTreeId))
                .collect(),
        )
    }

    fn lookup_py(
        &self,
        py: Python,
        repo: String,
        ids: Vec<AnyId>,
    ) -> PyResult<Serde<Vec<(usize, UploadToken)>>> {
        let responses = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    // Deduplicates input, keeping indexes
                    let indexable_ids: HashMap<IndexableId, Vec<usize>> = ids
                        .iter()
                        .cloned()
                        .map(|id| IndexableId {
                            id,
                            bubble_id: None,
                        })
                        .enumerate()
                        .fold(HashMap::new(), |mut map, (idx, id)| {
                            map.entry(id).or_default().push(idx);
                            map
                        });
                    self.lookup_batch(
                        repo,
                        indexable_ids.iter().map(|(id, _)| id.id.clone()).collect(),
                        None,
                        None,
                    )
                    .await
                    .map(|responses| {
                        responses
                            .into_iter()
                            .flat_map(|r| match r.result {
                                LookupResult::Present(token) => indexable_ids
                                    .get(&token.indexable_id())
                                    .cloned()
                                    .unwrap_or_default()
                                    .into_iter()
                                    .map(|idx| (idx, token.clone()))
                                    .collect(),
                                LookupResult::NotPresent(_) => vec![],
                            })
                            .collect()
                    })
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;
        Ok(Serde(responses))
    }

    fn uploadfileblobs_py(
        &self,
        py: Python,
        store: PyObject,
        repo: String,
        keys: Vec<(PyPathBuf /* path */, Serde<HgId> /* hgid */)>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<UploadToken>>>, PyFuture)> {
        let keys = to_keys(py, &keys)?;
        let store = as_legacystore(py, store)?;
        let downcast_error = "incorrect upload token, failed to downcast 'token.data.id' to 'AnyId::AnyFileContentId::ContentId' type";

        // Preupload LFS blobs
        store
            .upload(
                &keys
                    .iter()
                    .map(|key| StoreKey::hgid(key.clone()))
                    .collect::<Vec<_>>(),
            )
            .map_pyerr(py)?;

        let (content_ids, mut data): (Vec<_>, Vec<_>) = keys
            .into_iter()
            .map(|key| {
                let content = store.get_file_content(&key).map_pyerr(py)?;
                match content {
                    Some(v) => {
                        let content_id = calc_contentid(&v);
                        Ok((content_id, (content_id, v)))
                    }
                    None => Err(format_err!(
                        "failed to fetch file content for the key '{}'",
                        key
                    ))
                    .map_pyerr(py),
                }
            })
            // fail the entire operation if content is missing for some key because this is unexpected
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .unzip();

        // Deduplicate upload data
        let mut uniques = BTreeSet::new();
        data.retain(|(content_id, _)| uniques.insert(*content_id));
        let data = data
            .into_iter()
            .map(|(content_id, data)| (AnyFileContentId::ContentId(content_id), data))
            .collect();

        let (responses, stats) = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let response = self.process_files_upload(repo, data, None, None).await?;
                    let file_content_tokens = response
                        .entries
                        .try_collect::<Vec<_>>()
                        .await?
                        .into_iter()
                        .map(|token| {
                            let content_id = match token.data.id {
                                AnyId::AnyFileContentId(AnyFileContentId::ContentId(id)) => id,
                                _ => bail!(EdenApiError::Other(format_err!(downcast_error))),
                            };
                            Ok((content_id, token))
                        })
                        .collect::<Result<BTreeMap<_, _>, _>>()?;
                    let tokens = content_ids.into_iter().map(move |cid|
                        Result::<_, EdenApiError>::Ok(
                            file_content_tokens
                            .get(&cid)
                            .ok_or_else(|| EdenApiError::Other(format_err!("unexpected error: upload token is missing for ContentId({})", cid)))?.clone(),
                        )
                    );
                    Ok::<_, EdenApiError>((stream::iter(tokens), response.stats))
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        let responses_py = responses.map_ok(Serde).map_err(Into::into);
        let stats_py = PyFuture::new(py, stats.map_ok(PyStats))?;
        Ok((responses_py.into(), stats_py))
    }

    fn uploadfiles_py(
        &self,
        py: Python,
        store: PyObject,
        repo: String,
        keys: Vec<(
            PyPathBuf,   /* path */
            Serde<HgId>, /* hgid */
            Serde<HgId>, /* p1 */
            Serde<HgId>, /* p2 */
        )>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<UploadToken>>>, PyFuture)> {
        let keys = to_keys_with_parents(py, &keys)?;
        let store = as_legacystore(py, store)?;

        // Preupload LFS blobs
        store
            .upload(
                &keys
                    .iter()
                    .map(|(key, _)| StoreKey::hgid(key.clone()))
                    .collect::<Vec<_>>(),
            )
            .map_pyerr(py)?;

        let (mut upload_data, filenodes_data): (Vec<_>, Vec<_>) = keys
            .into_iter()
            .map(|(key, parents)| {
                let content = store.get(StoreKey::hgid(key.clone())).map_pyerr(py)?;
                match content {
                    StoreResult::Found(raw_content) => {
                        let raw_content = raw_content.into();
                        let (raw_data, copy_from) =
                            separate_metadata(&raw_content).map_pyerr(py)?;
                        let content_id = calc_contentid(&raw_data);
                        Ok((
                            (content_id, raw_data),
                            (key.hgid, content_id, parents, copy_from),
                        ))
                    }
                    _ => Err(EdenApiError::Other(format_err!(
                        "failed to fetch file content for the key '{}'",
                        key
                    )))
                    .map_pyerr(py),
                }
            })
            // fail the entire operation if content is missing for some key (filenode) because this is unexpected
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .unzip();

        // Deduplicate upload data
        let mut uniques = BTreeSet::new();
        upload_data.retain(|(content_id, _)| uniques.insert(*content_id));
        let upload_data = upload_data
            .into_iter()
            .map(|(content_id, data)| (AnyFileContentId::ContentId(content_id), data))
            .collect();

        let (responses, stats) = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let downcast_error = "incorrect upload token, failed to downcast 'token.data.id' to 'AnyId::AnyFileContentId::ContentId' type";
                    // upload file contents first, receiving upload tokens
                    let file_content_tokens = self
                        .process_files_upload(repo.clone(), upload_data, None, None)
                        .await?
                        .entries
                        .try_collect::<Vec<_>>()
                        .await?
                        .into_iter()
                        .map(|token| {
                            let content_id = match token.data.id {
                                AnyId::AnyFileContentId(AnyFileContentId::ContentId(id)) => id,
                                _ => bail!(EdenApiError::Other(format_err!(downcast_error))),
                            };
                            Ok((content_id, token))
                        })
                        .collect::<Result<BTreeMap<_, _>, _>>()?;

                    // build the list of HgFilenodeData for upload
                    let filenodes_data = filenodes_data.into_iter().map(|(node_id, content_id, parents, copy_from)| {
                        let file_content_upload_token = file_content_tokens.get(&content_id).ok_or_else(|| EdenApiError::Other(format_err!("unexpected error: upload token is missing for ContentId({})", content_id)))?.clone();
                        Ok(HgFilenodeData {
                            node_id,
                            parents,
                            file_content_upload_token,
                            metadata: copy_from.to_vec(),
                        })
                    }).collect::<Result<Vec<_>, EdenApiError>>()?;

                    // upload hg filenodes
                    let response = self
                        .upload_filenodes_batch(repo, filenodes_data)
                        .await?;

                    Ok::<_, EdenApiError>((response.entries, response.stats))
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        let responses_py = responses.map_ok(|r| Serde(r.token)).map_err(Into::into);
        let stats_py = PyFuture::new(py, stats.map_ok(PyStats))?;
        Ok((responses_py.into(), stats_py))
    }

    /// Upload trees
    fn uploadtrees_py(
        &self,
        py: Python,
        repo: String,
        items: Vec<(
            Serde<HgId>, /* hgid */
            Serde<HgId>, /* p1 */
            Serde<HgId>, /* p2 */
            PyBytes,     /* data */
        )>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<UploadToken>>>, PyFuture)> {
        let items = to_trees_upload_items(py, &items)?;
        let (responses, stats) = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let response = self.upload_trees_batch(repo, items).await?;
                    Ok::<_, EdenApiError>((response.entries, response.stats))
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        let responses_py = responses.map_ok(|r| Serde(r.token)).map_err(Into::into);
        let stats_py = PyFuture::new(py, stats.map_ok(PyStats))?;
        Ok((responses_py.into(), stats_py))
    }

    /// Upload changesets
    /// This method sends a single request, batching should be done outside.
    fn uploadchangesets_py(
        &self,
        py: Python,
        repo: String,
        changesets: Vec<(
            HgId,               /* hgid (node_id) */
            HgChangesetContent, /* changeset content */
        )>,
        mutations: Vec<Serde<HgMutationEntryContent>>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<UploadToken>>>, PyFuture)> {
        let changesets = changesets
            .into_iter()
            .map(|(node_id, content)| UploadHgChangeset {
                node_id,
                changeset_content: content,
            })
            .collect();
        let mutations = mutations.into_iter().map(|m| m.0).collect();
        let (responses, stats) = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let response = self.upload_changesets(repo, changesets, mutations).await?;
                    Ok::<_, EdenApiError>((response.entries, response.stats))
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        let responses_py = responses.map_ok(|r| Serde(r.token)).map_err(Into::into);
        let stats_py = PyFuture::new(py, stats.map_ok(PyStats))?;
        Ok((responses_py.into(), stats_py))
    }

    fn fetchsnapshot_py(
        &self,
        py: Python,
        repo: String,
        data: Serde<FetchSnapshotRequest>,
    ) -> PyResult<Serde<FetchSnapshotResponse>> {
        py.allow_threads(|| {
            block_unless_interrupted(async move {
                let cs_id = data.0.cs_id;
                self.fetch_snapshot(repo, data.0)
                    .await?
                    .entries
                    .next()
                    .await
                    .with_context(|| format_err!("Failed to find snapshot {}", cs_id))?
            })
        })
        .map_pyerr(py)?
        .map_pyerr(py)
        .map(Serde)
    }

    fn downloadfiles_py(
        &self,
        py: Python,
        repo: String,
        root: Serde<RepoPathBuf>,
        files: Vec<(PyPathBuf, Serde<UploadToken>)>,
    ) -> PyResult<bool> {
        let files = files
            .into_iter()
            .map(|(p, t)| Ok((to_path(py, &p)?, t.0)))
            .collect::<Result<Vec<_>, PyErr>>()?;
        py.allow_threads(|| block_unless_interrupted(download_files(self, &repo, &root.0, files)))
            .map_pyerr(py)?
            .map_pyerr(py)
            .map(|_| true)
    }

    fn checkfiles_py(
        &self,
        py: Python,
        root: Serde<RepoPathBuf>,
        files: Vec<(PyPathBuf, Serde<UploadToken>)>,
    ) -> PyResult<Vec<PyPathBuf>> {
        let files = files
            .into_iter()
            .map(|(p, t)| Ok((to_path(py, &p)?, t.0)))
            .collect::<Result<Vec<_>, PyErr>>()?;
        py.allow_threads(|| block_unless_interrupted(check_files(&root.0, files)))
            .map_pyerr(py)?
            .map_pyerr(py)
            .map(|v| v.into_iter().map(Into::into).collect())
    }

    fn downloadfiletomemory_py(
        &self,
        py: Python,
        repo: String,
        token: Serde<UploadToken>,
    ) -> PyResult<PyBytes> {
        py.allow_threads(|| block_unless_interrupted(self.download_file(repo.clone(), token.0)))
            .map_pyerr(py)?
            .map_pyerr(py)
            .map(|data| PyBytes::new(py, &data))
    }
}

impl<T: EdenApi + ?Sized> EdenApiPyExt for T {}

async fn write_trees(
    mut response: Response<Result<TreeEntry, EdenApiServerError>>,
    store: Arc<dyn HgIdMutableDeltaStore>,
    prog: Arc<ProgressBar>,
) -> Result<Stats, EdenApiError> {
    while let Some(Ok(entry)) = response.entries.try_next().await? {
        store.add_tree(&entry)?;
        prog.increase_position(1);
    }
    response.stats.await
}
