/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Context;
use cpython::*;
use futures::prelude::*;
use tokio::runtime::Runtime;

use cpython_ext::{PyPathBuf, ResultPyErrExt};
use edenapi::{EdenApi, EdenApiBlocking, EdenApiError, Fetch, Stats};
use edenapi_types::{FileEntry, HistoryEntry, TreeEntry};
use revisionstore::{HgIdMutableDeltaStore, HgIdMutableHistoryStore};

use crate::stats::stats;
use crate::util::{
    as_deltastore, as_historystore, meta_to_dict, to_hgids, to_keys, to_path, wrap_callback,
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
        progress: Option<PyObject>,
    ) -> PyResult<stats> {
        let keys = to_keys(py, &keys)?;
        let store = as_deltastore(py, store)?;
        let progress = progress.map(wrap_callback);

        let stats = py
            .allow_threads(|| {
                let mut rt = Runtime::new().context("Failed to initialize Tokio runtime")?;
                rt.block_on(async move {
                    let response = self.files(repo, keys, progress).await?;
                    write_file(response, store).await
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
        progress: Option<PyObject>,
    ) -> PyResult<stats> {
        let keys = to_keys(py, &keys)?;
        let store = as_historystore(py, store)?;
        let progress = progress.map(wrap_callback);

        let stats = py
            .allow_threads(|| {
                let mut rt = Runtime::new().context("Failed to initialize Tokio runtime")?;
                rt.block_on(async move {
                    let response = self.history(repo, keys, length, progress).await?;
                    write_history(response, store).await
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
        progress: Option<PyObject>,
    ) -> PyResult<stats> {
        let keys = to_keys(py, &keys)?;
        let store = as_deltastore(py, store)?;
        let progress = progress.map(wrap_callback);

        let stats = py
            .allow_threads(|| {
                let mut rt = Runtime::new().context("Failed to initialize Tokio runtime")?;
                rt.block_on(async move {
                    let response = self.trees(repo, keys, progress).await?;
                    write_tree(response, store).await
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
        progress: Option<PyObject>,
    ) -> PyResult<stats> {
        let rootdir = to_path(py, &rootdir)?;
        let mfnodes = to_hgids(py, mfnodes);
        let basemfnodes = to_hgids(py, basemfnodes);
        let store = as_deltastore(py, store)?;
        let progress = progress.map(wrap_callback);

        let stats = py
            .allow_threads(|| {
                let mut rt = Runtime::new().context("Failed to initialize Tokio runtime")?;
                rt.block_on(async move {
                    let response = self
                        .complete_trees(repo, rootdir, mfnodes, basemfnodes, depth, progress)
                        .await?;
                    write_tree(response, store).await
                })
            })
            .map_pyerr(py)?;

        stats::new(py, stats)
    }
}

impl<T: EdenApi + ?Sized> EdenApiPyExt for T {}

async fn write_file(
    mut response: Fetch<FileEntry>,
    store: Arc<dyn HgIdMutableDeltaStore>,
) -> Result<Stats, EdenApiError> {
    while let Some(entry) = response.entries.try_next().await? {
        store.add_file(&entry)?;
    }
    response.stats.await
}

async fn write_tree(
    mut response: Fetch<TreeEntry>,
    store: Arc<dyn HgIdMutableDeltaStore>,
) -> Result<Stats, EdenApiError> {
    while let Some(entry) = response.entries.try_next().await? {
        store.add_tree(&entry)?;
    }
    response.stats.await
}

async fn write_history(
    mut response: Fetch<HistoryEntry>,
    store: Arc<dyn HgIdMutableHistoryStore>,
) -> Result<Stats, EdenApiError> {
    while let Some(entry) = response.entries.try_next().await? {
        store.add_entry(&entry)?;
    }
    response.stats.await
}
