/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use cpython::*;
use futures::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, format_err, Context};
use async_runtime::block_unless_interrupted;
use cpython_async::PyFuture;
use cpython_async::TStream;
use cpython_ext::convert::Serde;
use cpython_ext::PyCell;
use cpython_ext::{PyPathBuf, ResultPyErrExt};
use dag_types::{Location, VertexName};
use edenapi::{EdenApi, EdenApiBlocking, EdenApiError, Response, Stats};
use edenapi_ext::{calc_contentid, download_files, upload_snapshot};
use edenapi_types::{
    AnyFileContentId, AnyId, CommitGraphEntry, CommitHashLookupResponse,
    CommitHashToLocationResponse, CommitKnownResponse, CommitLocationToHashRequest,
    CommitLocationToHashResponse, CommitRevlogData, EdenApiServerError, FetchSnapshotRequest,
    FetchSnapshotResponse, FileEntry, HgChangesetContent, HgFilenodeData, HgMutationEntryContent,
    HistoryEntry, LookupResponse, SnapshotRawData, TreeEntry, UploadHgChangeset,
    UploadSnapshotResponse, UploadToken, UploadTokensResponse, UploadTreeResponse,
};
use futures::stream;
use progress::{ProgressBar, ProgressFactory, Unit};
use pyrevisionstore::as_legacystore;
use revisionstore::{
    datastore::separate_metadata, HgIdMutableDeltaStore, HgIdMutableHistoryStore, StoreKey,
    StoreResult,
};
use types::RepoPathBuf;

use crate::pytypes::PyStats;
use crate::stats::stats;
use crate::util::{
    as_deltastore, as_historystore, meta_to_dict, to_contentid, to_hgid, to_hgids, to_keys,
    to_keys_with_parents, to_path, to_tree_attrs, to_trees_upload_items, wrap_callback,
};

/// Extension trait allowing EdenAPI methods to be called from Python code.
///
/// One nice benefit of making this a trait instead of directly implementing
/// the methods inside a `py_class!` macro invocation is that tools like
/// `rustfmt` can still parse the code.
pub trait EdenApiPyExt: EdenApi {
    fn health_py(&self, py: Python) -> PyResult<PyDict> {
        let meta = self.health_blocking().map_pyerr(py)?;
        meta_to_dict(py, &meta)
    }

    fn files_py(
        self: Arc<Self>,
        py: Python,
        store: PyObject,
        repo: String,
        keys: Vec<(PyPathBuf, PyBytes)>,
        callback: Option<PyObject>,
        progress: Arc<dyn ProgressFactory>,
    ) -> PyResult<stats> {
        let keys = to_keys(py, &keys)?;
        let store = as_deltastore(py, store)?;
        let callback = callback.map(wrap_callback);

        let stats = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let prog = progress.bar(
                        "Downloading files over HTTP",
                        Some(keys.len() as u64),
                        Unit::Named("files"),
                    )?;
                    let response = self.files(repo, keys, callback).await?;
                    write_files(response, store, prog.as_ref()).await
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        stats::new(py, stats)
    }

    fn history_py(
        self: Arc<Self>,
        py: Python,
        store: PyObject,
        repo: String,
        keys: Vec<(PyPathBuf, PyBytes)>,
        length: Option<u32>,
        callback: Option<PyObject>,
        progress: Arc<dyn ProgressFactory>,
    ) -> PyResult<stats> {
        let keys = to_keys(py, &keys)?;
        let store = as_historystore(py, store)?;
        let callback = callback.map(wrap_callback);

        let stats = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let prog = progress.bar(
                        "Downloading file history over HTTP",
                        Some(keys.len() as u64),
                        Unit::Named("entries"),
                    )?;
                    let response = self.history(repo, keys, length, callback).await?;
                    write_history(response, store, prog.as_ref()).await
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        stats::new(py, stats)
    }

    fn storetrees_py(
        self: Arc<Self>,
        py: Python,
        store: PyObject,
        repo: String,
        keys: Vec<(PyPathBuf, PyBytes)>,
        attributes: Option<PyDict>,
        callback: Option<PyObject>,
        progress: Arc<dyn ProgressFactory>,
    ) -> PyResult<stats> {
        let keys = to_keys(py, &keys)?;
        let store = as_deltastore(py, store)?;
        let callback = callback.map(wrap_callback);
        let attributes = attributes
            .as_ref()
            .map(|a| to_tree_attrs(py, a))
            .transpose()?;

        let stats = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let prog = progress.bar(
                        "Downloading trees over HTTP",
                        Some(keys.len() as u64),
                        Unit::Named("trees"),
                    )?;
                    let response = self.trees(repo, keys, attributes, callback).await?;
                    write_trees(response, store, prog.as_ref()).await
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        stats::new(py, stats)
    }

    fn trees_py(
        self: Arc<Self>,
        py: Python,
        repo: String,
        keys: Vec<(PyPathBuf, PyBytes)>,
        attributes: Option<PyDict>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<TreeEntry>>>, PyFuture)> {
        let keys = to_keys(py, &keys)?;
        let attributes = attributes
            .as_ref()
            .map(|a| to_tree_attrs(py, a))
            .transpose()?;

        let (trees, stats) = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let response = self.trees(repo, keys, attributes, None).await?;
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

    fn complete_trees_py(
        self: Arc<Self>,
        py: Python,
        store: PyObject,
        repo: String,
        rootdir: PyPathBuf,
        mfnodes: Vec<PyBytes>,
        basemfnodes: Vec<PyBytes>,
        depth: Option<usize>,
        callback: Option<PyObject>,
        progress: Arc<dyn ProgressFactory>,
    ) -> PyResult<stats> {
        let rootdir = to_path(py, &rootdir)?;
        let mfnodes = to_hgids(py, mfnodes);
        let basemfnodes = to_hgids(py, basemfnodes);
        let store = as_deltastore(py, store)?;
        let callback = callback.map(wrap_callback);

        let stats = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let prog = progress.bar(
                        "Downloading complete trees over HTTP",
                        None,
                        Unit::Named("trees"),
                    )?;
                    let response = self
                        .complete_trees(repo, rootdir, mfnodes, basemfnodes, depth, callback)
                        .await?;
                    write_trees(response, store, prog.as_ref()).await
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        stats::new(py, stats)
    }

    fn commit_revlog_data_py(
        self: Arc<Self>,
        py: Python,
        repo: String,
        nodes: Vec<PyBytes>,
        callback: Option<PyObject>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<CommitRevlogData>>>, PyFuture)> {
        let nodes = to_hgids(py, nodes);
        let callback = callback.map(wrap_callback);

        let (commits, stats) = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let response = self.commit_revlog_data(repo, nodes, callback).await?;
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

    fn bookmarks_py(
        self: Arc<Self>,
        py: Python,
        repo: String,
        bookmarks: Vec<String>,
        callback: Option<PyObject>,
    ) -> PyResult<(PyDict, stats)> {
        let callback = callback.map(wrap_callback);
        let response = self
            .bookmarks_blocking(repo, bookmarks, callback)
            .map_pyerr(py)?;

        let bookmarks = PyDict::new(py);
        for entry in response.entries.into_iter() {
            bookmarks.set_item(py, entry.bookmark, entry.hgid.map(|id| id.to_hex()))?;
        }
        let stats = stats::new(py, response.stats)?;
        Ok((bookmarks, stats))
    }

    fn hash_lookup_py(
        self: Arc<Self>,
        py: Python,
        repo: String,
        hash_prefixes: Vec<String>,
    ) -> PyResult<(
        TStream<anyhow::Result<Serde<CommitHashLookupResponse>>>,
        PyFuture,
    )> {
        let (responses, stats) = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let response = self.hash_prefixes_lookup(repo, hash_prefixes).await?;
                    let responses = response.entries;
                    let stats = response.stats;
                    Ok::<_, EdenApiError>((responses, stats))
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;
        let responses_py = responses.map_ok(Serde).map_err(Into::into);
        let stats_py = PyFuture::new(py, stats.map_ok(PyStats))?;
        Ok((responses_py.into(), stats_py))
    }

    fn commit_location_to_hash_py(
        self: Arc<Self>,
        py: Python,
        repo: String,
        requests: Vec<(PyBytes, u64, u64)>,
        callback: Option<PyObject>,
    ) -> PyResult<(
        TStream<anyhow::Result<Serde<CommitLocationToHashResponse>>>,
        PyFuture,
    )> {
        let callback = callback.map(wrap_callback);
        let requests = requests
            .into_iter()
            .map(|(hgid_bytes, distance, count)| {
                let location = Location::new(to_hgid(py, &hgid_bytes), distance);
                CommitLocationToHashRequest { location, count }
            })
            .collect();
        let (responses, stats) = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let response = self
                        .commit_location_to_hash(repo, requests, callback)
                        .await?;
                    Ok::<_, EdenApiError>((response.entries, response.stats))
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        let responses_py = responses.map_ok(Serde).map_err(Into::into);
        let stats_py = PyFuture::new(py, stats.map_ok(PyStats))?;
        Ok((responses_py.into(), stats_py))
    }

    fn commit_hash_to_location_py(
        self: Arc<Self>,
        py: Python,
        repo: String,
        master_heads: Vec<PyBytes>,
        hgids: Vec<PyBytes>,
        callback: Option<PyObject>,
    ) -> PyResult<(
        TStream<anyhow::Result<Serde<CommitHashToLocationResponse>>>,
        PyFuture,
    )> {
        let callback = callback.map(wrap_callback);
        let master_heads = to_hgids(py, master_heads);
        let hgids = to_hgids(py, hgids);
        let (responses, stats) = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let response = self
                        .commit_hash_to_location(repo, master_heads, hgids, callback)
                        .await?;
                    Ok::<_, EdenApiError>((response.entries, response.stats))
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        let responses_py = responses.map_ok(Serde).map_err(Into::into);
        let stats_py = PyFuture::new(py, stats.map_ok(PyStats))?;
        Ok((responses_py.into(), stats_py))
    }

    fn commit_known_py(
        self: Arc<Self>,
        py: Python,
        repo: String,
        hgids: Vec<PyBytes>,
    ) -> PyResult<(
        TStream<anyhow::Result<Serde<CommitKnownResponse>>>,
        PyFuture,
    )> {
        let hgids = to_hgids(py, hgids);
        let (responses, stats) = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let response = self.commit_known(repo, hgids).await?;
                    Ok::<_, EdenApiError>((response.entries, response.stats))
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        let responses_py = responses.map_ok(Serde).map_err(Into::into);
        let stats_py = PyFuture::new(py, stats.map_ok(PyStats))?;
        Ok((responses_py.into(), stats_py))
    }

    fn commit_graph_py(
        self: Arc<Self>,
        py: Python,
        repo: String,
        heads: Vec<PyBytes>,
        common: Vec<PyBytes>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<CommitGraphEntry>>>, PyFuture)> {
        let heads = to_hgids(py, heads);
        let common = to_hgids(py, common);
        let (responses, stats) = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let response = self.commit_graph(repo, heads, common).await?;
                    Ok::<_, EdenApiError>((response.entries, response.stats))
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        let responses_py = responses.map_ok(Serde).map_err(Into::into);
        let stats_py = PyFuture::new(py, stats.map_ok(PyStats))?;
        Ok((responses_py.into(), stats_py))
    }

    /// Get the "CloneData" serialized using mincode.
    fn clone_data_py(self: Arc<Self>, py: Python, repo: String) -> PyResult<PyBytes> {
        let bytes = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    match self.clone_data(repo).await {
                        Err(e) => Err(e),
                        Ok(data) => Ok(mincode::serialize(&data)),
                    }
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?
            .map_pyerr(py)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Get pll data for master pull fast path
    fn pull_fast_forward_master_py(
        self: Arc<Self>,
        py: Python,
        repo: String,
        old_master: PyBytes,
        new_master: PyBytes,
    ) -> PyResult<PyCell> {
        let old_master = to_hgid(py, &old_master);
        let new_master = to_hgid(py, &new_master);
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
        self: Arc<Self>,
        py: Python,
        repo: String,
        ids: Vec<PyBytes>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<LookupResponse>>>, PyFuture)> {
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
        self: Arc<Self>,
        py: Python,
        repo: String,
        nodes: Vec<PyBytes>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<LookupResponse>>>, PyFuture)> {
        self.lookup_py(
            py,
            repo,
            nodes
                .into_iter()
                .map(|node| AnyId::HgChangesetId(to_hgid(py, &node)))
                .collect(),
        )
    }

    fn lookup_filenodes(
        self: Arc<Self>,
        py: Python,
        repo: String,
        ids: Vec<PyBytes>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<LookupResponse>>>, PyFuture)> {
        self.lookup_py(
            py,
            repo,
            ids.into_iter()
                .map(|id| AnyId::HgFilenodeId(to_hgid(py, &id)))
                .collect(),
        )
    }

    fn lookup_trees(
        self: Arc<Self>,
        py: Python,
        repo: String,
        ids: Vec<PyBytes>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<LookupResponse>>>, PyFuture)> {
        self.lookup_py(
            py,
            repo,
            ids.into_iter()
                .map(|id| AnyId::HgTreeId(to_hgid(py, &id)))
                .collect(),
        )
    }

    fn lookup_filenodes_and_trees(
        self: Arc<Self>,
        py: Python,
        repo: String,
        filenodes_ids: Vec<PyBytes>,
        trees_ids: Vec<PyBytes>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<LookupResponse>>>, PyFuture)> {
        self.lookup_py(
            py,
            repo,
            filenodes_ids
                .into_iter()
                .map(|id| AnyId::HgFilenodeId(to_hgid(py, &id)))
                .chain(
                    trees_ids
                        .into_iter()
                        .map(|id| AnyId::HgTreeId(to_hgid(py, &id))),
                )
                .collect(),
        )
    }

    fn lookup_py(
        self: Arc<Self>,
        py: Python,
        repo: String,
        ids: Vec<AnyId>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<LookupResponse>>>, PyFuture)> {
        let (responses, stats) = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let response = self.lookup_batch(repo, ids, None).await?;
                    Ok::<_, EdenApiError>((response.entries, response.stats))
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        let responses_py = responses.map_ok(Serde).map_err(Into::into);
        let stats_py = PyFuture::new(py, stats.map_ok(PyStats))?;
        Ok((responses_py.into(), stats_py))
    }

    fn uploadfileblobs_py(
        self: Arc<Self>,
        py: Python,
        store: PyObject,
        repo: String,
        keys: Vec<(PyPathBuf /* path */, PyBytes /* hgid */)>,
    ) -> PyResult<(
        TStream<anyhow::Result<Serde<UploadTokensResponse>>>,
        PyFuture,
    )> {
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
                    let response = self.process_files_upload(repo, data, None).await?;
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
                    let tokens = content_ids.into_iter().enumerate().map(move |(idx, cid)|
                        Result::<_, EdenApiError>::Ok(UploadTokensResponse {
                            index: idx,
                            token: file_content_tokens
                            .get(&cid)
                            .ok_or_else(|| EdenApiError::Other(format_err!("unexpected error: upload token is missing for ContentId({})", cid)))?.clone(),
                        })
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
        self: Arc<Self>,
        py: Python,
        store: PyObject,
        repo: String,
        keys: Vec<(
            PyPathBuf, /* path */
            PyBytes,   /* hgid */
            PyBytes,   /* p1 */
            PyBytes,   /* p2 */
        )>,
    ) -> PyResult<(
        TStream<anyhow::Result<Serde<UploadTokensResponse>>>,
        PyFuture,
    )> {
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
                        .process_files_upload(repo.clone(), upload_data, None)
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

        let responses_py = responses.map_ok(Serde).map_err(Into::into);
        let stats_py = PyFuture::new(py, stats.map_ok(PyStats))?;
        Ok((responses_py.into(), stats_py))
    }

    /// Upload trees
    fn uploadtrees_py(
        self: Arc<Self>,
        py: Python,
        repo: String,
        items: Vec<(
            PyBytes, /* hgid */
            PyBytes, /* p1 */
            PyBytes, /* p2 */
            PyBytes, /* data */
        )>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<UploadTreeResponse>>>, PyFuture)> {
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

        let responses_py = responses.map_ok(Serde).map_err(Into::into);
        let stats_py = PyFuture::new(py, stats.map_ok(PyStats))?;
        Ok((responses_py.into(), stats_py))
    }

    /// Upload changesets
    /// This method sends a single request, batching should be done outside.
    fn uploadchangesets_py(
        self: Arc<Self>,
        py: Python,
        repo: String,
        changesets: Vec<(
            PyBytes,                   /* hgid (node_id) */
            Serde<HgChangesetContent>, /* changeset content */
        )>,
        mutations: Vec<Serde<HgMutationEntryContent>>,
    ) -> PyResult<(
        TStream<anyhow::Result<Serde<UploadTokensResponse>>>,
        PyFuture,
    )> {
        let changesets = changesets
            .into_iter()
            .map(|(node_id, content)| UploadHgChangeset {
                node_id: to_hgid(py, &node_id),
                changeset_content: content.0,
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

        let responses_py = responses.map_ok(Serde).map_err(Into::into);
        let stats_py = PyFuture::new(py, stats.map_ok(PyStats))?;
        Ok((responses_py.into(), stats_py))
    }

    fn uploadsnapshot_py(
        &self,
        py: Python,
        repo: String,
        data: Serde<SnapshotRawData>,
    ) -> PyResult<Serde<UploadSnapshotResponse>> {
        py.allow_threads(|| block_unless_interrupted(upload_snapshot(self, repo, data.0)))
            .map_pyerr(py)?
            .map_pyerr(py)
            .map(Serde)
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
}

impl<T: EdenApi + ?Sized> EdenApiPyExt for T {}

async fn write_files(
    mut response: Response<FileEntry>,
    store: Arc<dyn HgIdMutableDeltaStore>,
    prog: &dyn ProgressBar,
) -> Result<Stats, EdenApiError> {
    while let Some(entry) = response.entries.try_next().await? {
        store.add_file(&entry)?;
        prog.increment(1)?;
    }
    response.stats.await
}

async fn write_trees(
    mut response: Response<Result<TreeEntry, EdenApiServerError>>,
    store: Arc<dyn HgIdMutableDeltaStore>,
    prog: &dyn ProgressBar,
) -> Result<Stats, EdenApiError> {
    while let Some(Ok(entry)) = response.entries.try_next().await? {
        store.add_tree(&entry)?;
        prog.increment(1)?;
    }
    response.stats.await
}

async fn write_history(
    mut response: Response<HistoryEntry>,
    store: Arc<dyn HgIdMutableHistoryStore>,
    prog: &dyn ProgressBar,
) -> Result<Stats, EdenApiError> {
    while let Some(entry) = response.entries.try_next().await? {
        store.add_entry(&entry)?;
        prog.increment(1)?;
    }
    response.stats.await
}
