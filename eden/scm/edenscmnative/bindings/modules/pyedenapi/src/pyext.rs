/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use cpython::*;
use futures::prelude::*;

use async_runtime::block_on_future;
use cpython_async::PyFuture;
use cpython_async::TStream;
use cpython_ext::convert::Serde;
use cpython_ext::{PyPathBuf, ResultPyErrExt};
use dag_types::Location;
use edenapi::{EdenApi, EdenApiBlocking, EdenApiError, Fetch, Stats};
use edenapi_types::{
    CommitHashToLocationResponse, CommitLocationToHashRequest, CommitLocationToHashResponse,
    CommitRevlogData, EdenApiServerError, FileEntry, HistoryEntry, TreeEntry,
};
use progress::{ProgressBar, ProgressFactory, Unit};
use revisionstore::{HgIdMutableDeltaStore, HgIdMutableHistoryStore};

use crate::pytypes::PyStats;
use crate::stats::stats;
use crate::util::{
    as_deltastore, as_historystore, meta_to_dict, to_hgid, to_hgids, to_keys, to_path,
    to_tree_attrs, wrap_callback,
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
                block_on_future(async move {
                    let prog = progress.bar(
                        "Downloading files over HTTP",
                        Some(keys.len() as u64),
                        Unit::Named("files"),
                    )?;
                    let response = self.files(repo, keys, callback).await?;
                    write_files(response, store, prog.as_ref()).await
                })
            })
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
                block_on_future(async move {
                    let prog = progress.bar(
                        "Downloading file history over HTTP",
                        Some(keys.len() as u64),
                        Unit::Named("entries"),
                    )?;
                    let response = self.history(repo, keys, length, callback).await?;
                    write_history(response, store, prog.as_ref()).await
                })
            })
            .map_pyerr(py)?;

        stats::new(py, stats)
    }

    fn trees_py(
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
                block_on_future(async move {
                    let prog = progress.bar(
                        "Downloading trees over HTTP",
                        Some(keys.len() as u64),
                        Unit::Named("trees"),
                    )?;
                    let response = self.trees(repo, keys, attributes, callback).await?;
                    write_trees(response, store, prog.as_ref()).await
                })
            })
            .map_pyerr(py)?;

        stats::new(py, stats)
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
                block_on_future(async move {
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
                block_on_future(async move {
                    let response = self.commit_revlog_data(repo, nodes, callback).await?;
                    let commits = response.entries;
                    let stats = response.stats;
                    Ok::<_, EdenApiError>((commits, stats))
                })
            })
            .map_pyerr(py)?;

        let commits_py = commits.map_ok(Serde).map_err(Into::into);
        let stats_py = PyFuture::new(py, stats.map_ok(PyStats))?;
        Ok((commits_py.into(), stats_py))
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
                block_on_future(async move {
                    let response = self
                        .commit_location_to_hash(repo, requests, callback)
                        .await?;
                    Ok::<_, EdenApiError>((response.entries, response.stats))
                })
            })
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
                block_on_future(async move {
                    let response = self
                        .commit_hash_to_location(repo, master_heads, hgids, callback)
                        .await?;
                    Ok::<_, EdenApiError>((response.entries, response.stats))
                })
            })
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
