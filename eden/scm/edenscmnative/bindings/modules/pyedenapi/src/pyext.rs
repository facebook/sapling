/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use cpython::*;
use futures::prelude::*;

use anyhow::format_err;
use async_runtime::block_unless_interrupted;
use cpython_async::PyFuture;
use cpython_async::TStream;
use cpython_ext::convert::Serde;
use cpython_ext::{PyPathBuf, ResultPyErrExt};
use dag_types::Location;
use edenapi::{EdenApi, EdenApiBlocking, EdenApiError, Fetch, Stats};
use edenapi_types::CommitGraphEntry;
use edenapi_types::CommitKnownResponse;
use edenapi_types::{
    AnyFileContentId, AnyId, CommitHashToLocationResponse, CommitLocationToHashRequest,
    CommitLocationToHashResponse, CommitRevlogData, EdenApiServerError, FileEntry, HistoryEntry,
    LookupResponse, TreeEntry, UploadToken,
};
use progress::{ProgressBar, ProgressFactory, Unit};
use pyrevisionstore::as_legacystore;
use revisionstore::{HgIdMutableDeltaStore, HgIdMutableHistoryStore};

use crate::pytypes::PyStats;
use crate::stats::stats;
use crate::util::{
    as_deltastore, as_historystore, calc_contentid, meta_to_dict, to_contentid, to_hgid, to_hgids,
    to_keys, to_path, to_tree_attrs, wrap_callback,
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
                    match self.clone_data(repo, None).await {
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
    ) -> PyResult<PyBytes> {
        let old_master = to_hgid(py, &old_master);
        let new_master = to_hgid(py, &new_master);
        let bytes = {
            py.allow_threads(|| {
                block_unless_interrupted(async move {
                    match self
                        .pull_fast_forward_master(repo, old_master, new_master)
                        .await
                    {
                        Err(e) => Err(e),
                        Ok(data) => Ok(mincode::serialize(&data)),
                    }
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?
            .map_pyerr(py)?
        };
        Ok(PyBytes::new(py, &bytes))
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

    fn uploadfiles_py(
        self: Arc<Self>,
        py: Python,
        store: PyObject,
        repo: String,
        keys: Vec<(PyPathBuf, PyBytes)>,
        callback: Option<PyObject>,
        _progress: Arc<dyn ProgressFactory>,
    ) -> PyResult<(TStream<anyhow::Result<Serde<UploadToken>>>, PyFuture)> {
        let keys = to_keys(py, &keys)?;
        let store = as_legacystore(py, store)?;
        let callback = callback.map(wrap_callback);

        let data = keys
            .into_iter()
            .map(|key| {
                let content = store.get_file_content(&key).map_pyerr(py)?;
                match content {
                    Some(v) => {
                        // TODO: store the hashes that have been already calculated (key -> content_id mapping)
                        let content_id = calc_contentid(&v);
                        Ok((AnyFileContentId::ContentId(content_id), v))
                    }
                    None => Err(format_err!(
                        "failed to fetch file content for the key '{}'",
                        key
                    ))
                    .map_pyerr(py),
                }
            })
            // fail the entire operation if content is missing for some key because this is unexpected
            .collect::<Result<Vec<_>, _>>()?;

        let (responses, stats) = py
            .allow_threads(|| {
                block_unless_interrupted(async move {
                    let response = self.process_files_upload(repo, data, callback).await?;
                    Ok::<_, EdenApiError>((response.entries, response.stats))
                })
            })
            .map_pyerr(py)?
            .map_pyerr(py)?;

        let responses_py = responses.map_ok(Serde).map_err(Into::into);
        let stats_py = PyFuture::new(py, stats.map_ok(PyStats))?;
        Ok((responses_py.into(), stats_py))
    }
}

impl<T: EdenApi + ?Sized> EdenApiPyExt for T {}

async fn write_files(
    mut response: Fetch<FileEntry>,
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
    mut response: Fetch<Result<TreeEntry, EdenApiServerError>>,
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
    mut response: Fetch<HistoryEntry>,
    store: Arc<dyn HgIdMutableHistoryStore>,
    prog: &dyn ProgressBar,
) -> Result<Stats, EdenApiError> {
    while let Some(entry) = response.entries.try_next().await? {
        store.add_entry(&entry)?;
        prog.increment(1)?;
    }
    response.stats.await
}
